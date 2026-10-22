//! The container used from outside the crate, as CADCraft's `.cad` uses it:
//! its own records in named sections, the header saying whose they are.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use cad_native::codec::Level;
use cad_native::container::{Builder, Container, Encoding};
use cad_native::tags::{put_record, records, TagReader, TagWriter};
use cad_native::{records_of, RECORDS_CADCRAFT};

fn file(compress: bool, recovery: bool) -> Vec<u8> {
    let mut entities = Vec::new();
    for (i, name) in ["line", "circle"].iter().enumerate() {
        let mut w = TagWriter::new();
        w.str(0, name);
        w.f64(10, i as f64 * 2.5);
        put_record(&mut entities, w);
    }
    let mut b = Builder::new(Encoding::current(compress, Level::Default), recovery).for_records(RECORDS_CADCRAFT);
    b.section("Meta", b"cadcraft 0.10.0".to_vec());
    b.records("Entities", entities);
    b.finish(1, 0)
}

#[test]
fn another_programs_records_round_trip_through_the_container() {
    for (compress, recovery) in [(true, true), (false, false), (true, false)] {
        let bytes = file(compress, recovery);
        assert_eq!(records_of(&bytes), Some(RECORDS_CADCRAFT));
        let c = Container::parse(&bytes).unwrap();
        assert_eq!(c.header.records, RECORDS_CADCRAFT);
        let (meta, ok) = c.read(c.entry("Meta").unwrap()).unwrap();
        assert!(ok);
        assert_eq!(meta, b"cadcraft 0.10.0");
        let (raw, ok) = c.read(c.entry("Entities").unwrap()).unwrap();
        assert!(ok);
        let recs = records(&raw).unwrap();
        assert_eq!(recs.len(), 2);
        let mut r = TagReader::new(recs[1]);
        assert_eq!(r.str(0).unwrap(), "circle");
        assert_eq!(r.f64(10).unwrap(), 2.5);
    }
}

#[test]
fn blackcad_refuses_them_with_a_message() {
    let e = cad_native::read_bytes(&file(true, true)).unwrap_err().to_string();
    assert!(e.contains("CADCraft drawing (.cad)"), "{e}");
}

#[test]
fn a_damaged_front_header_still_says_whose_records() {
    let mut bytes = file(true, true);
    bytes[20] ^= 0xFF;
    assert_eq!(records_of(&bytes), Some(RECORDS_CADCRAFT));
    assert_eq!(Container::parse(&bytes).unwrap().header.records, RECORDS_CADCRAFT);
}
