//! Helpers for producing the per-register hex payloads consumed by the
//! wallet's `additionalRegisters` field on a `PaymentRequestDto`.
//!
//! Each helper returns the hex of one register's wire bytes — i.e., the
//! same shape Scala's `ValueSerializer` emits inside the on-chain
//! register block (per-register: sigma constant header + value bytes,
//! or a `CreateTuple` / `ConcreteCollection` expression for the
//! non-constant cases). The decoder on the node side
//! ([`ergo_ser::register::read_register_value`]) round-trips these
//! verbatim.
//!
//! The encoding goes through the node's own `ergo_ser::register`
//! writers (path-dep), so the bytes here are produced by exactly the
//! same code the on-chain decoder reads from. Hand-rolling the
//! type-code + zig-zag VLQ + tuple-opcode dispatch would silently
//! diverge on any of the harder shapes.

use ergo_primitives::writer::VlqWriter;
use ergo_ser::register::{write_registers, AdditionalRegisters, RegisterValue};
use ergo_ser::sigma_type::SigmaType;
use ergo_ser::sigma_value::{CollValue, SigmaValue};

/// Per-register payload hex for a `Long` value (constant form, type
/// code `0x05` followed by ZigZag-VLQ value bytes).
pub fn slong_hex(value: i64) -> String {
    payload_hex(RegisterValue {
        tpe: SigmaType::SLong,
        value: SigmaValue::Long(value),
    })
}

/// Per-register payload hex for a `(Long, Long)` tuple. Emitted as a
/// `CreateTuple` expression (opcode `0x86`) to match the on-chain
/// encoding for tuple-typed registers.
pub fn slong_pair_hex(a: i64, b: i64) -> String {
    payload_hex(RegisterValue {
        tpe: SigmaType::STuple(vec![SigmaType::SLong, SigmaType::SLong]),
        value: SigmaValue::Tuple(vec![SigmaValue::Long(a), SigmaValue::Long(b)]),
    })
}

/// Per-register payload hex for a `Coll[Byte]` (the byte slice is
/// copied into the register value). Emitted as the constant form
/// (type code `0x0e` for `Coll[SByte]` followed by VLQ length + raw
/// bytes).
pub fn coll_byte_hex(bytes: &[u8]) -> String {
    payload_hex(RegisterValue {
        tpe: SigmaType::SColl(Box::new(SigmaType::SByte)),
        value: SigmaValue::Coll(CollValue::Bytes(bytes.to_vec())),
    })
}

/// Serialize a single-register block and return the per-register
/// payload hex (drops the leading count byte that
/// [`write_registers`] prepends).
fn payload_hex(reg: RegisterValue) -> String {
    let block = AdditionalRegisters {
        registers: vec![reg],
    };
    let mut w = VlqWriter::new();
    write_registers(&mut w, &block).expect("write_registers");
    let bytes = w.result();
    // First byte is the register-count (1); strip it so the result is
    // exactly the per-register payload the wallet decoder consumes.
    hex::encode(&bytes[1..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ergo_primitives::reader::VlqReader;
    use ergo_ser::register::read_registers;

    /// Reframe a single per-register hex back into a 1-register
    /// `AdditionalRegisters` block so we can decode it.
    fn parse_single(hex_payload: &str) -> RegisterValue {
        let payload = hex::decode(hex_payload).unwrap();
        let mut framed = Vec::with_capacity(payload.len() + 1);
        framed.push(1u8);
        framed.extend_from_slice(&payload);
        let mut r = VlqReader::new(&framed);
        let regs = read_registers(&mut r).unwrap();
        assert!(r.is_empty());
        assert_eq!(regs.registers.len(), 1);
        regs.registers.into_iter().next().unwrap()
    }

    #[test]
    fn slong_round_trips() {
        let cases: &[i64] = &[0, 1, -1, i64::MAX, i64::MIN, 100_000_000_000_000];
        for &v in cases {
            let hex = slong_hex(v);
            let reg = parse_single(&hex);
            assert_eq!(reg.tpe, SigmaType::SLong);
            assert_eq!(reg.value, SigmaValue::Long(v));
        }
    }

    #[test]
    fn slong_pair_round_trips() {
        let hex = slong_pair_hex(0, 0);
        let reg = parse_single(&hex);
        assert_eq!(
            reg.tpe,
            SigmaType::STuple(vec![SigmaType::SLong, SigmaType::SLong])
        );
        assert_eq!(
            reg.value,
            SigmaValue::Tuple(vec![SigmaValue::Long(0), SigmaValue::Long(0)])
        );

        let hex = slong_pair_hex(1_000_000, 42);
        let reg = parse_single(&hex);
        assert_eq!(
            reg.value,
            SigmaValue::Tuple(vec![SigmaValue::Long(1_000_000), SigmaValue::Long(42)])
        );
    }

    #[test]
    fn coll_byte_zero_padded_round_trips() {
        let hex = coll_byte_hex(&[0u8; 32]);
        let reg = parse_single(&hex);
        assert_eq!(reg.tpe, SigmaType::SColl(Box::new(SigmaType::SByte)));
        assert_eq!(reg.value, SigmaValue::Coll(CollValue::Bytes(vec![0; 32])));
    }

    #[test]
    fn coll_byte_arbitrary_round_trips() {
        let bytes: Vec<u8> = (0..64).map(|i| (i * 7) as u8).collect();
        let hex = coll_byte_hex(&bytes);
        let reg = parse_single(&hex);
        assert_eq!(reg.value, SigmaValue::Coll(CollValue::Bytes(bytes)));
    }
}
