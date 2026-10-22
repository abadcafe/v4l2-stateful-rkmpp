//! module: hevc_irap_tests; responsibility: IRAP reference admission
//! through the public syntax interface. operations: inspect, seek,
//! boundary; entities: none; tests: decode/output boundaries,
//! current/following, ST/LT, leading pictures and reset.
use codec_parser::header::{Header, Hevc as HeaderHevc, Long, Short};

use super::codec::Syntax;
use super::hevc::Hevc;

fn header(poc: u32, kind: u8, short: Vec<Short>) -> Header {
  let bytes = include_bytes!("../../tests/artifacts/hevc-yuv420p.hevc");
  let first =
    codec_parser::Validator::new(u32::from_le_bytes(*b"HEVC"))
      .unwrap()
      .validate(&bytes[..5496], 1)
      .unwrap()
      .remove(0);
  let Some(Header::Hevc(first)) = first.header else {
    panic!("missing HEVC header")
  };
  Header::Hevc(HeaderHevc {
    sequence: first.sequence,
    kind,
    temporal_id: 0,
    no_prior: false,
    display: true,
    lsb: poc,
    short,
    long: vec![],
  })
}

fn hidden(mut header: Header) -> Header {
  let Header::Hevc(picture) = &mut header else {
    panic!("missing HEVC header")
  };
  picture.display = false;
  header
}

fn reference(delta: i32, used: bool) -> Vec<Short> {
  vec![Short { delta, used }]
}

fn requests(
  poc: u32,
  kind: u8,
  target: u32,
  used: bool,
) -> [Header; 2] {
  let short = header(
    poc,
    kind,
    reference(
      i32::try_from(target).unwrap() - i32::try_from(poc).unwrap(),
      used,
    ),
  );
  let mut long = header(poc, kind, vec![]);
  let Header::Hevc(picture) = &mut long else {
    panic!("missing HEVC header")
  };
  picture.long.push(Long { lsb: target, cycle: Some(0), used });
  [short, long]
}

fn seed(syntax: &mut Hevc, old_poc: u32) -> u64 {
  syntax.inspect(&[], Some(&header(0, 19, vec![]))).unwrap().unwrap();
  let old = header(
    old_poc,
    1,
    reference(-i32::try_from(old_poc).unwrap(), true),
  );
  // A non-output reference may follow the later CRA in output order;
  // this isolates the decoding-order restriction from the POC test.
  let old = if old_poc > 4 { hidden(old) } else { old };
  syntax.inspect(&[], Some(&old)).unwrap().unwrap();
  syntax
    .inspect(
      &[],
      Some(&header(
        4,
        21,
        reference(i32::try_from(old_poc).unwrap() - 4, false),
      )),
    )
    .unwrap()
    .unwrap()
    .id
}

#[test]
fn test_inspect_trailing_rps_cannot_cross_irap_decode_or_output_boundary()
 {
  for (old, leading) in [(2, false), (2, true), (6, false)] {
    for used in [false, true] {
      for request in requests(8, 1, if leading { 3 } else { old }, used)
      {
        let mut syntax = Hevc::default();
        let irap = seed(&mut syntax, old);
        let previous = if leading {
          syntax
            .inspect(
              &[],
              Some(&header(
                3,
                9,
                vec![
                  Short { delta: -1, used: true },
                  Short { delta: 1, used: false },
                ],
              )),
            )
            .unwrap()
            .unwrap()
            .id
        } else {
          irap
        };
        assert!(syntax.inspect(&[], Some(&request)).is_err());
        let retry = syntax
          .inspect(&[], Some(&header(8, 1, reference(-4, true))))
          .unwrap()
          .unwrap();
        assert_eq!(retry.id, previous + 1);
        assert_eq!(retry.references, [irap, retry.id]);
      }
    }
  }
}

#[test]
fn test_inspect_radl_pre_irap_reference_is_only_allowed_as_following() {
  for used in [false, true] {
    for request in requests(3, 7, 2, used) {
      let mut syntax = Hevc::default();
      let irap = seed(&mut syntax, 2);
      assert_eq!(syntax.inspect(&[], Some(&request)).is_err(), used);
      if used {
        let retry = syntax
          .inspect(&[], Some(&header(3, 7, reference(1, true))))
          .unwrap()
          .unwrap();
        assert_eq!(retry.references, [irap, retry.id]);
      }
    }
    for request in requests(3, 9, 2, used) {
      let mut syntax = Hevc::default();
      seed(&mut syntax, 2);
      syntax.inspect(&[], Some(&request)).unwrap().unwrap();
    }
  }
}

#[test]
fn test_inspect_cra_rps_cannot_precede_an_earlier_irap() {
  for used in [false, true] {
    for target in [2, 4] {
      for request in requests(6, 21, target, used) {
        let mut syntax = Hevc::default();
        seed(&mut syntax, 2);
        assert_eq!(
          syntax.inspect(&[], Some(&request)).is_err(),
          target == 2
        );
      }
    }
  }
}

#[test]
fn test_inspect_cra_checks_all_preceding_iraps_even_when_not_output() {
  for target in [3, 5] {
    for request in requests(6, 21, target, false) {
      let mut syntax = Hevc::default();
      // PicOutputFlag=0 does not erase an IRAP from the 8.3.2 RPS
      // restriction, which refers to any preceding IRAP picture.
      for picture in [
        header(0, 19, vec![]),
        hidden(header(4, 21, vec![])),
        header(2, 21, vec![]),
        header(target, 1, vec![]),
      ] {
        syntax.inspect(&[], Some(&picture)).unwrap().unwrap();
      }
      assert_eq!(
        syntax.inspect(&[], Some(&request)).is_err(),
        target == 3
      );
    }
  }
}

#[test]
fn test_inspect_new_random_access_sequence_replaces_irap_boundaries() {
  for boundary in [false, true] {
    let mut syntax = Hevc::default();
    let previous = seed(&mut syntax, 2);
    if boundary {
      syntax.boundary();
    } else {
      syntax.seek();
    }
    let next = seed(&mut syntax, 2);
    assert!(next > previous);
    let output = syntax
      .inspect(&[], Some(&header(6, 1, reference(-2, true))))
      .unwrap()
      .unwrap();
    assert_eq!(output.references, [next, output.id]);
  }
}

#[test]
fn test_inspect_radl_cannot_use_rasl_but_can_retain_it() {
  for used in [false, true] {
    for request in requests(3, 7, 1, used) {
      let mut syntax = Hevc::default();
      let irap = seed(&mut syntax, 2);
      syntax
        .inspect(
          &[],
          Some(&header(
            1,
            9,
            vec![
              Short { delta: 1, used: true },
              Short { delta: 3, used: false },
            ],
          )),
        )
        .unwrap()
        .unwrap();
      assert_eq!(syntax.inspect(&[], Some(&request)).is_err(), used);
      if used {
        let retry = syntax
          .inspect(&[], Some(&header(3, 7, reference(1, true))))
          .unwrap()
          .unwrap();
        assert_eq!(retry.references, [irap, retry.id]);
      }
    }
  }
}
