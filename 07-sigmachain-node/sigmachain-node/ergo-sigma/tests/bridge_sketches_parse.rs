//! Smoke test for the trustless-peg contract sketches.
//!
//! Each tree was compiled via the Scala Ergo node 6.1.2 (`/script/p2sAddress`,
//! treeVersion=3). We don't evaluate them here — just confirm YOLO's Rust
//! `read_ergo_tree` can parse the wire bytes. That closes the loop on
//! Scala compiler -> Rust deserializer compatibility for the v6 method calls
//! the contracts use (`Header.checkPow`, `Global.decodeNbits`,
//! `Global.deserializeTo[Header]`, `AvlTree.insert`, `AvlTree.get`, etc.).

use ergo_primitives::reader::VlqReader;
use ergo_ser::ergo_tree::read_ergo_tree;

const RELAY_TREE_HEX: &str = "100804000402041004000410050201010580897ad811d601dc6a04dd01e4e3010e68d602db68037201d603e4c6a7070ed604b2a5730000d605db68017201d6069ae4c6a706047301d607860272057a7e720605d608e4c6a70464d609e4c6a70806d60adc6a07dd01db68087201d60be4c6a70564d60ce4dc640a720b027202e4e3030ed60d9a7bb4720c7302b1720c720ad60ee4c672040564d60f9a7cb4720c730373047305d610e4e3070ed611eded93db6401720edb6401e4dc640c720b0283013c0e0e86027205b37a720f7210e4e3060e93db6402720edb6402720b937b7210720dd1edededdb68107201959372027203d801d612e4c672040464edededed93db64017212db6401e4dc640c72080283013c0e0e7207e4e3020e93db64027212db6402720893e4c672040604720693e4c67204070e720593e4c6720408069a7209720a730695ed91720d72099472027203d801d612e4c672040464ededededed721193db64017212db6401e4dc640ce4e304640283013c0e0e7207e4e3050e93db64027212db6402720893e4c6720406047d720f0493e4c67204070e720593e4c672040806720d7211eded92c1720499c1a7730793db63087204db6308a793c27204c2a7";

const LOCK_TREE_HEX: &str = "100b04000e2000000000000000000000000000000000000000000000000000000000000000020402040004000e20000000000000000000000000000000000000000000000000000000000000000104020400053c0e2000000000000000000000000000000000000000000000000000000000000000030400d808d601b2db6501fe730000d6027301d603dc6a04dd01e4e3020e68d604e4e30063d605c57204d606c1a7d607b2a5730200d608b2a5730300d1ededededededed938cb2db63087201730400017305938cb2db6308b2a473060073070001720292997ee4c672010604057ce4dc640ae4c67201046402db68017203e4e3030e730893e4dc640adb68057203027205e4e3010ec3720493cbc27204730993e4c6720405057206ed938cb2db63087207730a0001720293e4c67207050e7205ed93c27208e4c67204040e92c172087206";

const MINT_TREE_HEX: &str = "101404000e200000000000000000000000000000000000000000000000000000000000000002040404000e200000000000000000000000000000000000000000000000000000000000000005040204000e20000000000000000000000000000000000000000000000000000000000000000104020400053c0e20000000000000000000000000000000000000000000000000000000000000000404000400040004020402040204000400d80ad601b2db6501fe730000d6027301d603dc6a04dd01e4e3020e68d604e4e30063d605c57204d606b2a5730200d607b2a5730300d6087304d609c17204d60ab2a5730500d1ededededededed938cb2db63087201730600017307938cb2db6308b2a473080073090001720292997ee4c672010604057ce4dc640ae4c67201046402db68017203e4e3030e730a93e4dc640adb68057203027205e4e3010ec3720493cbc27204730bed938cb2db63087206730c0001720293e4c67206050e7205edededed93c27207c2a793b2db63087207730d00b2db6308a7730e00938cb2db63087207730f0001720893998cb2db6308a7731000028cb2db6308720773110002720992c17207c1a7eded93c2720ae4c67204040e938cb2db6308720a731200017208938cb2db6308720a731300027209";

/// `sigmaProp(false)` — minimal unspendable receipt with R4/R5/R6 metadata
/// set by the burn tx.
const BURN_TREE_HEX: &str = "10010100d17300";

/// DSP / nullifier box — AVL set of consumed foreign-chain box ids.
const DSP_TREE_HEX: &str = "100504020400040004040400d805d601b2a5730000d602db6308a7d6038cb2720273010001d60495938cb2db630872017302000172037201d801d604b2a573030095938cb2db630872047304000172037204a7d605e4c67204050ed1ededed93c17204c1a793db63087204720293c27204c2a793db6401e4c672040464db6401e4dc640ce4c6a704640283013c0e0e860272057205e4e3000e";

/// Control: a v6 tree that uses Global.decodeNbits but NOT SHeader.
/// If this parses while the bridge sketches fail, the gap is specifically
/// SHeader-value deserialization, not v6 trees in general.
const CONTROL_DECODE_NBITS_HEX: &str = "10020580890f060100d191dc6a07dd0173007301";

fn parse(name: &str, hex: &str) {
    let bytes = hex::decode(hex).expect("hex decode");
    let mut r = VlqReader::new(&bytes);
    let tree = read_ergo_tree(&mut r).unwrap_or_else(|e| panic!("{name} parse failed: {e:?}"));
    let remaining = bytes.len() - r.position();
    assert_eq!(remaining, 0, "{name}: {} trailing bytes after parse", remaining);
    let _ = tree;
    println!("{name}: parsed {} bytes OK", bytes.len());
}

fn parse_with_stack(name: &'static str, hex: &'static str) {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(move || parse(name, hex))
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn relay_sketch_parses() {
    parse_with_stack("relay-sketch", RELAY_TREE_HEX);
}

#[test]
fn control_decode_nbits_parses() {
    parse("control-decode-nbits", CONTROL_DECODE_NBITS_HEX);
}

#[test]
fn lock_sketch_parses() {
    parse_with_stack("lock-sketch", LOCK_TREE_HEX);
}

#[test]
fn mint_sketch_parses() {
    parse_with_stack("mint-sketch", MINT_TREE_HEX);
}

#[test]
fn burn_sketch_parses() {
    parse("burn-sketch", BURN_TREE_HEX);
}

#[test]
fn dsp_sketch_parses() {
    parse_with_stack("dsp-sketch", DSP_TREE_HEX);
}
