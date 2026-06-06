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

const LOCK_TREE_HEX: &str = "100904000402040204000e20000000000000000000000000000000000000000000000000000000000000000104000e200000000000000000000000000000000000000000000000000000000000000002053c0e200000000000000000000000000000000000000000000000000000000000000003d808d601db6501fed602b27201730000d603b27201730100d604db63087203d605dc6a04dd01e4e3020e68d606e4e30063d607c57206d608b2a5730200d1ededededededed938cb2db63087202730300017304938cb2720473050001730692997ee4c672020604057ce4dc640ae4c67202046402db68017205e4e3030e730793e4dc640adb68057205027207e4e3010ec3720693cbc27206730893e4c67206040ee4c6a7040e93e4c672060505e4c6a70505eded93db6401e4c672080464db6401e4dc640ce4c6720304640283013c0e0e860272077207e4e3040e93c27208c2720393db630872087204";

/// Control: a v6 tree that uses Global.decodeNbits but NOT SHeader.
/// If this parses while the bridge sketches fail, the gap is specifically
/// SHeader-value deserialization, not v6 trees in general.
const CONTROL_DECODE_NBITS_HEX: &str = "10020580890f060100d191dc6a07dd0173007301";

const MINT_TREE_HEX: &str = "101304000402040404000e200000000000000000000000000000000000000000000000000000000000000005040204000e20000000000000000000000000000000000000000000000000000000000000000104000e200000000000000000000000000000000000000000000000000000000000000002053c0e2000000000000000000000000000000000000000000000000000000000000000040400040004020402040204000400d80cd601db6501fed602b27201730000d603b27201730100d604db63087203d605dc6a04dd01e4e3020e68d606e4e30063d607c57206d608b2a5730200d609b2a5730300d60a7304d60be4c672060505d60cb2a5730500d1ededededededed938cb2db63087202730600017307938cb2720473080001730992997ee4c672020604057ce4dc640ae4c67202046402db68017205e4e3030e730a93e4dc640adb68057205027207e4e3010ec3720693cbc27206730beded93db6401e4c672080464db6401e4dc640ce4c6720304640283013c0e0e860272077207e4e3040e93c27208c2720393db630872087204edededed93c27209c2a793b2db63087209730c00b2db6308a7730d00938cb2db63087209730e0001720a93998cb2db6308a7730f00028cb2db6308720973100002720b92c17209c1a7eded93c2720ce4c67206040e938cb2db6308720c73110001720a938cb2db6308720c73120002720b";

fn parse(name: &str, hex: &str) {
    let bytes = hex::decode(hex).expect("hex decode");
    let mut r = VlqReader::new(&bytes);
    let tree = read_ergo_tree(&mut r).unwrap_or_else(|e| panic!("{name} parse failed: {e:?}"));
    let remaining = bytes.len() - r.position();
    assert_eq!(remaining, 0, "{name}: {} trailing bytes after parse", remaining);
    let _ = tree; // value is used to ensure the parse didn't get short-circuited
    println!("{name}: parsed {} bytes OK", bytes.len());
}

#[test]
fn relay_sketch_parses() {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| parse("relay-sketch", RELAY_TREE_HEX))
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn lock_sketch_parses() {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| parse("lock-sketch", LOCK_TREE_HEX))
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn control_decode_nbits_parses() {
    parse("control-decode-nbits", CONTROL_DECODE_NBITS_HEX);
}

#[test]
fn mint_sketch_parses() {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| parse("mint-sketch", MINT_TREE_HEX))
        .unwrap()
        .join()
        .unwrap();
}
