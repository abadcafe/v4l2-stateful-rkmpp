//! module: mpp; responsibility: expose owned MPP objects without
//! leaking C handles. operations: decoder and packet lifetimes;
//! entities: Decoder, Packet, Frame. tests: mpp_tests and public V4L2
//! codec/pixel tests.
mod api;
#[cfg(test)]
mod api_tests;
mod decoder;
#[cfg(test)]
mod decoder_tests;
mod frame;
#[cfg(test)]
mod frame_tests;
mod group;
#[cfg(test)]
mod group_tests;
mod input;
#[cfg(test)]
mod input_tests;
mod reservation;
#[cfg(test)]
mod reservation_tests;
pub use reservation::Reservation;
mod packet;
#[cfg(test)]
mod packet_tests;
pub use decoder::{Decoder, Submission};
pub use frame::{Frame, FrameInfo};
pub use packet::{Input, InputAccess, Packet};

mod status;
#[cfg(test)]
mod status_tests;
