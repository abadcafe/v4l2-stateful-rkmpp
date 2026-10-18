//! module: backend::output; responsibility: publish decoded images
//! through an owned CAPTURE transaction. Pool admission, GPU work and
//! display leases never escape this module. operations:
//! register/start/recycle/stop/push and status queries;
//! entities: Output, Status; tests: output_tests and service
//! acceptance.
use std::io;
use std::num::NonZeroU32;

use super::capture::{CapturePool, CopyReason, Presentation};
use super::frame::FrameInput;
use super::layout::Requirements;
use crate::dma::Buffer;
use crate::image::Depth;
use crate::mpp::{Decoder, FrameInfo};
use crate::protocol::{Message, Reply};
use crate::quantize::Quantizer;
mod format;
#[cfg(test)]
mod format_tests;
mod negotiation;
#[cfg(test)]
mod negotiation_tests;
use format::Format;
use negotiation::Negotiation;
struct Conversion {
  frame: FrameInput,
  index: usize,
  token: NonZeroU32,
  bytesused: u32,
  reason: Option<CopyReason>,
}
pub(super) struct Output {
  capture: CapturePool,
  format: Option<Format>,
  negotiation: Negotiation,
  quantizer: Option<Quantizer>,
  converting: Option<Conversion>,
  transfers: [u64; 5],
  capture_stalls: u64,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Status {
  Unconfigured,
  Waiting,
  Active,
  Stopped,
}
impl Output {
  pub(super) fn new() -> Self {
    Self {
      capture: CapturePool::default(),
      format: None,
      negotiation: Negotiation::default(),
      quantizer: None,
      converting: None,
      transfers: [0; 5],
      capture_stalls: 0,
    }
  }

  pub(super) fn id(&self) -> u32 {
    self.capture.registration().0
  }

  fn active(&self) -> bool {
    self.negotiation.active()
  }

  fn waiting(&self) -> bool {
    self.negotiation.waiting()
  }

  pub(super) fn status(&self) -> Status {
    if self.negotiation.paused() {
      Status::Stopped
    } else if self.active() {
      Status::Active
    } else if self.waiting() {
      Status::Waiting
    } else {
      Status::Unconfigured
    }
  }

  pub(super) fn requirements(&self) -> Option<Requirements> {
    self.format.as_ref().map(Format::requirements)
  }

  pub(super) fn register(
    &mut self,
    id: u32,
    buffers: Vec<Buffer>,
    decoder: Option<&Decoder>,
  ) -> io::Result<()> {
    if id == 0 || !(1..=32).contains(&buffers.len()) {
      return Err(io::Error::other("invalid CAPTURE registration"));
    }
    if self.capture.registration().0 != 0 {
      return Err(io::Error::other("unexpected capture registration"));
    }
    if decoder.is_some_and(Decoder::has_output)
      && self.format.as_ref().is_some_and(|f| {
        buffers.len() < f.message().count as usize
          || buffers
            .iter()
            .any(|b| b.size() < f.message().size as usize)
      })
    {
      self.negotiation.retire();
    }
    self.capture = CapturePool::new(id, buffers);
    Ok(())
  }

  pub(super) fn start(
    &mut self,
    count: usize,
    reuse: bool,
    mut decoder: Option<&mut Decoder>,
  ) -> io::Result<()> {
    let mut negotiation = self.negotiation.clone();
    negotiation.start(count, reuse)?;
    if reuse {
      if !self.waiting() {
        return Err(io::Error::other(
          "capture reuse without pending layout",
        ));
      }
      let format = self.format.as_ref().ok_or_else(|| {
        io::Error::other("capture reuse without geometry")
      })?;
      let layout = format.message();
      if count != self.capture.registration().1
        || !self
          .capture
          .fits(layout.count as usize, layout.size as usize)
      {
        return Err(io::Error::other(
          "CAPTURE reuse does not satisfy negotiated layout",
        ));
      }
      let decoder = decoder.as_deref_mut().ok_or_else(|| {
        io::Error::other("capture reuse without decoder")
      })?;
      let reservations = decoder.reuse_output(format.ten())?;
      self
        .capture
        .reuse(format.message().size as usize, reservations)?;
    }
    self.negotiation = negotiation;
    self.ready(decoder)
  }

  pub(super) fn recycle(
    &mut self,
    pool: u32,
    index: usize,
    token: NonZeroU32,
    decoder: Option<&mut Decoder>,
  ) -> io::Result<()> {
    if pool == 0 || pool != self.capture.registration().0 {
      return Err(io::Error::other("invalid recycle pool"));
    }
    self.capture.recycle(index, token, self.negotiation.retiring())?;
    self.ready(decoder)
  }

  fn ready(&mut self, decoder: Option<&mut Decoder>) -> io::Result<()> {
    if let Some(decoder) = decoder {
      if !self.negotiation.retiring()
        && self.format.as_ref().is_some_and(|f| !f.ten())
        && decoder.has_output()
      {
        decoder.import_capture(
          &self.capture.imports(),
          self.capture.registration().1,
        )?;
      }
      let acknowledge = self.negotiation.acknowledge();
      if self.negotiation.commit(self.capture.registration().1)
        && acknowledge
      {
        decoder.ready()?;
      }
    } else {
      self.negotiation.commit(self.capture.registration().1);
    }
    Ok(())
  }

  pub(super) fn stop(
    &mut self,
    decoder: Option<&mut Decoder>,
  ) -> io::Result<()> {
    self.quantizer = None;
    self.converting = None;
    self.capture = CapturePool::default();
    self.negotiation.stop();
    if let Some(decoder) = decoder {
      decoder.configure_output(
        self.format.as_ref().is_some_and(Format::ten),
      )?;
    }
    Ok(())
  }

  fn announce(
    &mut self,
    info: FrameInfo,
    requirements: Requirements,
    decoder: &mut Decoder,
  ) -> io::Result<Message> {
    let format = Format::new(info, requirements)?;
    let retiring =
      decoder.has_output() || self.capture.registration().0 != 0;
    if !retiring {
      decoder.configure_output(format.ten())?;
    }
    self.negotiation.announce(retiring, info.info_change);
    let message = format.message();
    self.format = Some(format);
    Ok(message)
  }

  fn frame_reply(
    &self,
    index: usize,
    token: NonZeroU32,
    info: FrameInfo,
    bytesused: u32,
  ) -> io::Result<Message> {
    Ok(Message {
      kind: Reply::Frame as u32,
      index: index as u32,
      count: token.get(),
      bytesused,
      timestamp: info.timestamp,
      field: info.field,
      flags: (if info.failed { Message::FRAME_ERROR } else { 0 })
        | crate::timestamp::Source::try_from(info.timestamp_source)?
          as u32,
      ..Default::default()
    })
  }

  pub(super) fn completion(&self) -> Option<crate::descriptor::Lease> {
    self.quantizer.as_ref().and_then(Quantizer::completion)
  }

  pub(super) fn idle(&self) -> bool {
    self.converting.is_none()
  }

  pub(super) fn take(&mut self) -> io::Result<Option<Message>> {
    if self.converting.is_none() {
      return Ok(None);
    }
    if !self.quantizer.as_mut().unwrap().finish()? {
      return Ok(None);
    }
    let done = self.converting.take().unwrap();
    self.transfers[match done.reason {
      None => 1,
      Some(CopyReason::Unregistered) => 2,
      Some(CopyReason::Held) => 3,
      Some(CopyReason::Foreign) => 4,
    }] += 1;
    self
      .frame_reply(
        done.index,
        done.token,
        done.frame.info,
        done.bytesused,
      )
      .map(Some)
  }

  pub(super) fn push(
    &mut self,
    frame: FrameInput,
    decoder: &mut Decoder,
  ) -> io::Result<(Option<FrameInput>, Option<Message>)> {
    if !self.idle() {
      return Ok((Some(frame), None));
    }
    let info = frame.info;
    let image = frame.frame.plane()?.is_some();
    let changed = image
      && self
        .format
        .as_ref()
        .is_some_and(|f| f.changed(info, frame.requirements));
    let ten = self.format.as_ref().is_some_and(Format::ten);
    if info.info_change || changed {
      let message = self.announce(info, frame.requirements, decoder)?;
      return Ok(((!info.info_change).then_some(frame), Some(message)));
    }
    if self.waiting() && image {
      return Ok((Some(frame), None));
    }
    if image {
      let layout = self
        .format
        .as_ref()
        .ok_or_else(|| {
          io::Error::other("decoded image without negotiated geometry")
        })?
        .message();
      let (frame, reason) = if !ten {
        match self.capture.present(frame.frame)? {
          Presentation::Direct(index, token) => {
            self.transfers[0] += 1;
            return Ok((
              None,
              Some(self.frame_reply(
                index,
                token,
                info,
                layout.stride * layout.rows * 3 / 2,
              )?),
            ));
          }
          Presentation::Convert(image, reason) => (
            FrameInput {
              frame: image,
              info,
              requirements: frame.requirements,
            },
            Some(reason),
          ),
        }
      } else {
        (frame, None)
      };
      let Some(target) = self.capture.prepare_write(
        decoder,
        layout.size as usize,
        layout.stride,
        layout.rows,
      )?
      else {
        self.capture_stalls += 1;
        return Ok((Some(frame), None));
      };
      if self.quantizer.is_none() {
        self.quantizer = Some(Quantizer::new()?);
      }
      self.quantizer.as_mut().unwrap().submit(
        if ten { Depth::Ten } else { Depth::Eight },
        layout.coded_width,
        layout.coded_height,
        frame.frame.plane()?.ok_or_else(|| {
          io::Error::other("conversion source missing")
        })?,
        target.plane(),
      )?;
      let (index, token) = target.commit();
      self.converting = Some(Conversion {
        frame,
        index,
        token,
        bytesused: layout.stride * layout.rows * 3 / 2,
        reason,
      });
      return Ok((None, None));
    }
    if info.failed && self.active() {
      let layout = self
        .format
        .as_ref()
        .ok_or_else(|| {
          io::Error::other("error frame without negotiated layout")
        })?
        .message();
      let Some(target) = self.capture.prepare_write(
        decoder,
        layout.size as usize,
        layout.stride,
        layout.rows,
      )?
      else {
        self.capture_stalls += 1;
        return Ok((Some(frame), None));
      };
      let (index, token) = target.commit();
      return Ok((
        None,
        Some(self.frame_reply(index, token, info, 0)?),
      ));
    }
    Ok((None, None))
  }
}

impl Drop for Output {
  fn drop(&mut self) {
    if self.transfers.iter().any(|&count| count != 0) {
      eprintln!(
        "rkmpp transfers: direct={} quantized={} unregistered_copy={} \
         held_copy={} foreign_copy={} capture_stalls={}",
        self.transfers[0],
        self.transfers[1],
        self.transfers[2],
        self.transfers[3],
        self.transfers[4],
        self.capture_stalls
      );
    }
    // GPU completion precedes release of decoder references and
    // CAPTURE.
    self.quantizer = None;
    self.converting = None;
  }
}
