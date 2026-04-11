use crate::arduino::{encoderRead, encoderReset};
use crate::CmdError;

#[derive(Debug, Clone)]
pub struct RotaryEncoder {
	pin_a: u8,
	pin_b: u8,
}

impl RotaryEncoder {
	pub fn new(pin_a: u8, pin_b: u8) -> Result<Self, CmdError> {
		encoderReset(pin_a, pin_b)?;
		Ok(Self { pin_a, pin_b })
	}

	pub fn pin_a(&self) -> u8 {
		self.pin_a
	}

	pub fn pin_b(&self) -> u8 {
		self.pin_b
	}

	pub fn read(&self) -> Result<i64, CmdError> {
		encoderRead(self.pin_a, self.pin_b)
	}

	pub fn reset(&self) -> Result<(), CmdError> {
		encoderReset(self.pin_a, self.pin_b)
	}
}

pub(crate) fn quadrature_state(a_high: bool, b_high: bool) -> u8 {
	((a_high as u8) << 1) | (b_high as u8)
}

pub(crate) fn quadrature_delta(previous_state: u8, current_state: u8) -> i64 {
	match ((previous_state & 0b11) << 2) | (current_state & 0b11) {
		0b0001 | 0b0111 | 0b1110 | 0b1000 => 1,
		0b0010 | 0b1011 | 0b1101 | 0b0100 => -1,
		_ => 0,
	}
}

#[cfg(test)]
mod tests {
	use super::quadrature_delta;

	#[test]
	fn forward_sequence_counts_up() {
		assert_eq!(quadrature_delta(0b00, 0b01), 1);
		assert_eq!(quadrature_delta(0b01, 0b11), 1);
		assert_eq!(quadrature_delta(0b11, 0b10), 1);
		assert_eq!(quadrature_delta(0b10, 0b00), 1);
	}

	#[test]
	fn reverse_sequence_counts_down() {
		assert_eq!(quadrature_delta(0b00, 0b10), -1);
		assert_eq!(quadrature_delta(0b10, 0b11), -1);
		assert_eq!(quadrature_delta(0b11, 0b01), -1);
		assert_eq!(quadrature_delta(0b01, 0b00), -1);
	}
}