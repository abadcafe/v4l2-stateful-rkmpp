//! module: hevc_temporal_tests; responsibility: temporal reference
//! admission through the public syntax interface. operations: inspect;
//! entities: none; tests: required/following, SLNR, TSA and STSA
//! barriers.
use std::rc::Rc;

use codec_parser::header::{Header, Hevc as HeaderHevc, Long, Short};
use codec_parser::syntax::hevc::Sequence;

use super::codec::Syntax;
use super::hevc::Hevc;

fn header(
  poc: u32,
  kind: u8,
  temporal_id: u8,
  short: Vec<Short>,
) -> Header {
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
    sequence: Rc::new(Sequence {
      geometry: first.sequence.geometry,
      poc_bits: first.sequence.poc_bits,
      temporal_nested: false,
      layers: vec![codec_parser::Limits::default(); 7],
    }),
    kind,
    temporal_id,
    no_prior: false,
    display: true,
    lsb: poc,
    short,
    long: vec![],
  })
}
fn long_header(
  poc: u32,
  kind: u8,
  temporal_id: u8,
  lsb: u32,
  used: bool,
) -> Header {
  let mut header = header(poc, kind, temporal_id, vec![]);
  let Header::Hevc(picture) = &mut header else {
    panic!("missing HEVC header")
  };
  picture.long.push(Long { lsb, cycle: Some(0), used });
  header
}
fn begin() -> Hevc {
  let mut syntax = Hevc::default();
  syntax
    .inspect(&[], Some(&header(0, 19, 0, vec![])))
    .unwrap()
    .unwrap();
  syntax
}
fn reference(delta: i32, used: bool) -> Vec<Short> {
  vec![Short { delta, used }]
}

fn nesting(mut header: Header, temporal_nested: bool) -> Header {
  let Header::Hevc(picture) = &mut header else {
    panic!("missing HEVC header")
  };
  picture.sequence = Rc::new(Sequence {
    geometry: picture.sequence.geometry,
    poc_bits: picture.sequence.poc_bits,
    temporal_nested,
    layers: picture.sequence.layers.clone(),
  });
  header
}

#[test]
fn test_inspect_temporal_id_is_within_the_active_sps_layers() {
  for declared in 1..=7 {
    for temporal_id in 0..=6 {
      let [first, current] = [
        header(0, 19, 0, vec![]),
        header(
          2,
          if temporal_id == 0 { 1 } else { 3 },
          temporal_id,
          reference(-2, true),
        ),
      ]
      .map(|mut header| {
        let Header::Hevc(picture) = &mut header else {
          panic!("missing HEVC header")
        };
        picture.sequence = Rc::new(Sequence {
          geometry: picture.sequence.geometry,
          poc_bits: picture.sequence.poc_bits,
          temporal_nested: true,
          layers: vec![codec_parser::Limits::default(); declared],
        });
        header
      });
      let mut syntax = Hevc::default();
      syntax.inspect(&[], Some(&first)).unwrap().unwrap();
      assert_eq!(
        syntax.inspect(&[], Some(&current)).is_err(),
        usize::from(temporal_id) >= declared,
      );
    }
  }
  let mut empty = header(0, 19, 0, vec![]);
  let Header::Hevc(picture) = &mut empty else {
    panic!("missing HEVC header")
  };
  picture.sequence = Rc::new(Sequence {
    geometry: picture.sequence.geometry,
    poc_bits: picture.sequence.poc_bits,
    temporal_nested: true,
    layers: vec![],
  });
  assert!(Hevc::default().inspect(&[], Some(&empty)).is_err());
}

#[test]
fn test_inspect_nested_reference_observes_intervening_lower_layers() {
  for nested in [false, true] {
    for used in [false, true] {
      for intervening_layer in [0, 2] {
        for request in [
          header(6, 3, 2, reference(-4, used)),
          long_header(6, 3, 2, 2, used),
        ] {
          let mut syntax = Hevc::default();
          for picture in [
            header(0, 19, 0, vec![]),
            header(2, 3, 1, reference(-2, true)),
            header(
              4,
              if intervening_layer == 0 { 1 } else { 3 },
              intervening_layer,
              reference(-2, false),
            ),
          ] {
            syntax
              .inspect(&[], Some(&nesting(picture, nested)))
              .unwrap()
              .unwrap();
          }
          assert_eq!(
            syntax
              .inspect(&[], Some(&nesting(request, nested)))
              .is_err(),
            nested && used && intervening_layer == 0
          );
          // A lower-layer picture before the candidate reference is
          // permitted, including after explicit prediction reset.
          syntax.seek();
          for picture in [
            header(0, 19, 0, vec![]),
            header(2, 3, 1, reference(-2, true)),
            header(4, 3, 2, reference(-2, true)),
          ] {
            syntax
              .inspect(&[], Some(&nesting(picture, nested)))
              .unwrap()
              .unwrap();
          }
        }
      }
    }
  }
}

#[test]
fn test_inspect_nested_high_layers_require_eligible_nal_types() {
  for nested in [false, true] {
    for kind in 0..=5 {
      let mut syntax = Hevc::default();
      syntax
        .inspect(&[], Some(&nesting(header(0, 19, 0, vec![]), nested)))
        .unwrap()
        .unwrap();
      assert_eq!(
        syntax
          .inspect(
            &[],
            Some(&nesting(
              header(2, kind, 1, reference(-2, true)),
              nested,
            )),
          )
          .is_err(),
        nested && !matches!(kind, 2 | 3)
      );
    }
  }
}

#[test]
fn test_inspect_required_reference_cannot_come_from_a_higher_temporal_layer()
 {
  for used in [false, true] {
    for request in [
      header(4, 1, 1, reference(-2, used)),
      long_header(4, 1, 1, 2, used),
    ] {
      let mut syntax = begin();
      syntax
        .inspect(&[], Some(&header(2, 1, 2, reference(-2, true))))
        .unwrap()
        .unwrap();
      assert_eq!(syntax.inspect(&[], Some(&request)).is_err(), used);
    }
  }
}

#[test]
fn test_inspect_slnr_can_be_used_only_by_higher_temporal_layers() {
  for (current_layer, used) in [(1, true), (2, true), (1, false)] {
    for request in [
      header(4, 1, current_layer, reference(-2, used)),
      long_header(4, 1, current_layer, 2, used),
    ] {
      let mut syntax = begin();
      syntax
        .inspect(&[], Some(&header(2, 0, 1, reference(-2, true))))
        .unwrap()
        .unwrap();
      assert_eq!(
        syntax.inspect(&[], Some(&request)).is_err(),
        current_layer == 1 && used
      );
    }
  }
}

#[test]
fn test_inspect_tsa_requires_lower_layer_for_both_current_and_following_rps()
 {
  for kind in [2, 3] {
    for layer in [1, 2] {
      for used in [false, true] {
        for request in [
          header(4, kind, layer, reference(-2, used)),
          long_header(4, kind, layer, 2, used),
        ] {
          let mut syntax = begin();
          syntax
            .inspect(&[], Some(&header(2, 1, 1, reference(-2, true))))
            .unwrap()
            .unwrap();
          assert_eq!(
            syntax.inspect(&[], Some(&request)).is_err(),
            layer == 1
          );
        }
      }
    }
  }
}

#[test]
fn test_inspect_stsa_disallows_same_layer_current_but_permits_following()
 {
  for kind in [4, 5] {
    for used in [false, true] {
      for request in [
        header(4, kind, 1, reference(-2, used)),
        long_header(4, kind, 1, 2, used),
      ] {
        let mut syntax = begin();
        syntax
          .inspect(&[], Some(&header(2, 1, 1, reference(-2, true))))
          .unwrap()
          .unwrap();
        assert_eq!(syntax.inspect(&[], Some(&request)).is_err(), used);
      }
    }
  }
}

#[test]
fn test_inspect_stsa_barrier_blocks_old_current_use_but_not_new_or_following()
 {
  for (delta, used) in [(-4, true), (-4, false), (-2, true)] {
    for request in [
      header(6, 1, 1, reference(delta, used)),
      long_header(6, 1, 1, (6 + delta) as u32, used),
    ] {
      let mut syntax = begin();
      syntax
        .inspect(&[], Some(&header(2, 1, 1, reference(-2, true))))
        .unwrap()
        .unwrap();
      syntax
        .inspect(&[], Some(&header(4, 5, 1, reference(-2, false))))
        .unwrap()
        .unwrap();
      assert_eq!(
        syntax.inspect(&[], Some(&request)).is_err(),
        delta == -4 && used
      );
    }
  }
}
