//! module: backend::output; responsibility: publish decoded images
//! through an owned CAPTURE transaction. Pool admission, GPU work and
//! display leases never escape this module. operations:
//! register/start/recycle/stop/push/take and status queries;
//! entities: Output, Status; tests: output_tests and service
//! acceptance.
use std::io;

use super::capture::{CapturePool, Presentation};
use super::frame::FrameInput;
use super::layout::Extent;
use crate::converter::{Converter, Job};
use crate::dma::Buffer;
use crate::mpp::{Decoder, FrameInfo};
use crate::notify::Notify;
use crate::protocol::{Message, Reply};
mod format;
#[cfg(test)]
mod format_tests;
mod negotiation;
#[cfg(test)]
mod negotiation_tests;
use format::Format;
use negotiation::Negotiation;
pub(super) struct Output {
  notify: Notify,
  capture: CapturePool,
  format: Option<Format>,
  negotiation: Negotiation,
  converter: Option<Converter<FrameInput>>,
  converting: usize,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Status {
  Unconfigured,
  Waiting,
  Active,
  Stopped,
}
impl Output {
  pub(super) fn new(notify: Notify) -> Self {
    Self {
      notify,
      capture: CapturePool::default(),
      format: None,
      negotiation: Negotiation::default(),
      converter: None,
      converting: 0,
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

  pub(super) fn idle(&self) -> bool {
    self.converting == 0
  }

  pub(super) fn extent(&self) -> Option<Extent> {
    self.format.as_ref().map(Format::extent)
  }

  pub(super) fn register(
    &mut self,
    id: u32,
    buffers: Vec<Buffer>,
    decoder: Option<&Decoder>,
  ) -> io::Result<()> {
    if self.capture.registration().0 != 0 {
      return Err(io::Error::other("unexpected capture registration"));
    }
    if decoder.is_some_and(Decoder::has_output)
      && self.format.as_ref().is_some_and(|f| {
        buffers.iter().any(|b| b.size() < f.message().size as usize)
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
    if reuse {
      if !self.waiting() {
        return Err(io::Error::other(
          "capture reuse without pending layout",
        ));
      }
      let format = self.format.as_ref().ok_or_else(|| {
        io::Error::other("capture reuse without geometry")
      })?;
      let decoder = decoder.as_deref_mut().ok_or_else(|| {
        io::Error::other("capture reuse without decoder")
      })?;
      let reservations = decoder.reuse_output(format.ten())?;
      if reservations.is_none() {
        self.capture.forget_imports();
      }
      self.capture.reuse(
        format.message().size as usize,
        reservations.unwrap_or_default(),
      )?;
    }
    self.negotiation.start(count, reuse)?;
    self.ready(decoder)
  }

  pub(super) fn recycle(
    &mut self,
    pool: u32,
    index: usize,
    decoder: Option<&mut Decoder>,
  ) -> io::Result<()> {
    if pool == 0 || pool != self.capture.registration().0 {
      return Err(io::Error::other("invalid recycle pool"));
    }
    self.capture.recycle(index, self.negotiation.retiring())?;
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
    self.converter = None;
    self.converting = 0;
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
    extent: Extent,
    decoder: &mut Decoder,
  ) -> io::Result<Message> {
    let format = Format::new(info, extent)?;
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
    info: FrameInfo,
    bytesused: u32,
  ) -> io::Result<Message> {
    Ok(Message {
      kind: Reply::Frame as u32,
      index: index as u32,
      bytesused,
      timestamp: info.timestamp,
      field: info.field,
      flags: (if info.failed { Message::FRAME_ERROR } else { 0 })
        | crate::timestamp::Source::try_from(info.timestamp_source)?
          as u32,
      ..Default::default()
    })
  }

  pub(super) fn take(&mut self) -> io::Result<Option<Message>> {
    let Some(completion) = self
      .converter
      .as_mut()
      .map(Converter::take)
      .transpose()?
      .flatten()
    else {
      return Ok(None);
    };
    let (index, frame, result) = completion.into_parts();
    result?;
    self.converting -= 1;
    let layout = self.format.as_ref().unwrap().message();
    self
      .frame_reply(
        index as usize,
        frame.info,
        layout.stride * layout.rows * 3 / 2,
      )
      .map(Some)
  }

  pub(super) fn push(
    &mut self,
    frame: FrameInput,
    decoder: &mut Decoder,
  ) -> io::Result<(Option<FrameInput>, Option<Message>)> {
    let info = frame.info;
    let image = frame.frame.plane()?.is_some();
    let changed = image
      && self
        .format
        .as_ref()
        .is_some_and(|f| f.changed(info, frame.extent));
    let ten = self.format.as_ref().is_some_and(Format::ten);
    if !self.idle() && (info.info_change || changed || !image || !ten) {
      return Ok((Some(frame), None));
    }
    if info.info_change || changed {
      let message = self.announce(info, frame.extent, decoder)?;
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
      let frame = if !ten {
        match self.capture.present(frame.frame)? {
          Presentation::Direct(index) => {
            return Ok((
              None,
              Some(self.frame_reply(
                index,
                info,
                layout.stride * layout.rows * 3 / 2,
              )?),
            ));
          }
          Presentation::Convert(image) => {
            FrameInput { frame: image, info, extent: frame.extent }
          }
        }
      } else {
        frame
      };
      let Some(target) = self.capture.prepare_write(
        decoder,
        layout.size as usize,
        layout.stride,
        layout.rows,
      )?
      else {
        return Ok((Some(frame), None));
      };
      if self.converter.is_none() {
        self.converter = Some(Converter::new(self.notify.clone())?);
      }
      let job = Job::new(
        target.index() as u32,
        (layout.coded_width, layout.coded_height),
        frame,
        target.image(),
      );
      if let Err(error) = self.converter.as_mut().unwrap().submit(job) {
        if !error.is_full() {
          return Err(io::Error::other("conversion worker stopped"));
        }
        return Ok((Some(error.into_job().into_input()), None));
      }
      target.commit();
      self.converting += 1;
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
        return Ok((Some(frame), None));
      };
      let index = target.commit();
      return Ok((None, Some(self.frame_reply(index, info, 0)?)));
    }
    Ok((None, None))
  }
}

impl Drop for Output {
  fn drop(&mut self) {
    self.converter = None;
  }
}
