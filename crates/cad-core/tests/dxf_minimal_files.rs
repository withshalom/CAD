//! DXF files as simple exporters write them, read by the DXF reader
//! (`cad-dxf`) and then drawn, edited and saved by the core.

mod minimal_files {
    //! DXF files as simple exporters write them, hand-written from the
    //! DXF reference: no handles, no tables, no subclass markers.

    use cad_core::dxf::read_bytes;
    use cad_core::geom::Vec3;
    use cad_core::model::*;
    use cad_core::render;

    fn dxf(lines: &[&str]) -> Vec<u8> {
        let mut s = lines.join("\n");
        s.push('\n');
        s.into_bytes()
    }

    fn drawn(d: &Drawing) -> Vec<String> {
        d.model_space_entities().iter().map(|o| o.type_name.clone()).collect()
    }

    #[test]
    fn entities_alone_are_drawn_and_editable() {
        let d = read_bytes(&dxf(&[
            "0", "SECTION", "2", "ENTITIES",
            "0", "LINE", "8", "0", "10", "0.0", "20", "0.0", "30", "0.0", "11", "10.0", "21", "10.0", "31", "0.0",
            "0", "CIRCLE", "8", "HOLES", "10", "5.0", "20", "5.0", "30", "0.0", "40", "2.0",
            "0", "ENDSEC", "0", "EOF",
        ]))
        .unwrap();
        assert_eq!(drawn(&d), ["LINE", "CIRCLE"]);
        let scene = render::model_space(&d);
        assert_eq!(scene.stats.entities_drawn, 2);
        let line = d.model_space_entities()[0];
        match &line.kind {
            ObjectKind::Line(l) => assert_eq!(l.end, Vec3::new(10.0, 10.0, 0.0)),
            k => panic!("{k:?}"),
        }
        let circle = d.model_space_entities()[1];
        assert!(matches!(&circle.kind, ObjectKind::Circle(c) if c.radius == 2.0));
        let holes = d.layer_by_name("HOLES").expect("made").0;
        assert_eq!(circle.entity.as_ref().unwrap().layer, holes);
        // Every handle its own.
        let mut hs: Vec<Handle> = d.objects.keys().copied().collect();
        hs.dedup();
        assert_eq!(hs.len(), d.objects.len());
        // It takes an edit, and saves and reads back as DWG.
        let mut d = d;
        d.add_entity(Handle::NULL, ObjectKind::Line(Line { end: Vec3::new(1.0, 0.0, 0.0), extrusion: Vec3::Z, ..Default::default() }))
            .unwrap();
        assert_eq!(drawn(&d).len(), 3);
        let dwg = cad_core::dwg::write::to_bytes(&d).unwrap().bytes;
        let back = cad_core::open_bytes(&dwg).unwrap();
        assert_eq!(drawn(&back).len(), 3);
    }

    #[test]
    fn entities_without_owners_go_to_model_space() {
        // Tables with handles, as AutoCAD 2000 and later write them; the
        // entities carry handles and markers but no owner.
        let d = read_bytes(&dxf(&[
            "0", "SECTION", "2", "TABLES",
            "0", "TABLE", "2", "LAYER", "5", "2", "70", "1",
            "0", "LAYER", "5", "10", "100", "AcDbSymbolTableRecord", "100", "AcDbLayerTableRecord", "2", "0", "70", "0", "62", "7", "6", "Continuous",
            "0", "ENDTAB",
            "0", "TABLE", "2", "BLOCK_RECORD", "5", "1", "70", "2",
            "0", "BLOCK_RECORD", "5", "1F", "100", "AcDbSymbolTableRecord", "100", "AcDbBlockTableRecord", "2", "*Model_Space",
            "0", "BLOCK_RECORD", "5", "1B", "100", "AcDbSymbolTableRecord", "100", "AcDbBlockTableRecord", "2", "*Paper_Space",
            "0", "ENDTAB",
            "0", "ENDSEC",
            "0", "SECTION", "2", "ENTITIES",
            "0", "LINE", "5", "30", "100", "AcDbEntity", "8", "0", "100", "AcDbLine", "10", "0", "20", "0", "30", "0", "11", "3", "21", "4", "31", "0",
            "0", "CIRCLE", "5", "31", "330", "77", "100", "AcDbEntity", "8", "0", "100", "AcDbCircle", "10", "1", "20", "1", "30", "0", "40", "1",
            "0", "ENDSEC", "0", "EOF",
        ]))
        .unwrap();
        // The circle names an owner the file does not have: model space too.
        assert_eq!(drawn(&d), ["LINE", "CIRCLE"]);
        assert_eq!(d.model_space(), Handle(0x1F));
        assert_eq!(render::model_space(&d).stats.entities_drawn, 2);
    }

    #[test]
    fn an_old_style_polyline_keeps_its_vertices() {
        let d = read_bytes(&dxf(&[
            "0", "SECTION", "2", "ENTITIES",
            "0", "POLYLINE", "8", "0", "66", "1", "70", "1",
            "0", "VERTEX", "8", "0", "10", "0", "20", "0",
            "0", "VERTEX", "8", "0", "10", "10", "20", "0",
            "0", "VERTEX", "8", "0", "10", "10", "20", "5",
            "0", "SEQEND", "8", "0",
            "0", "ENDSEC", "0", "EOF",
        ]))
        .unwrap();
        assert_eq!(drawn(&d), ["POLYLINE_2D"]);
        match &d.model_space_entities()[0].kind {
            ObjectKind::Polyline(p) => assert_eq!(p.vertices.len(), 3),
            k => panic!("{k:?}"),
        }
        assert_eq!(render::model_space(&d).stats.entities_drawn, 1);
    }

    #[test]
    fn found_but_unplaced_is_reported() {
        // Entity types the model does not hold: found, none placed.
        let d = read_bytes(&dxf(&[
            "0", "SECTION", "2", "ENTITIES",
            "0", "WIPEOUT", "8", "0",
            "0", "ENDSEC", "0", "EOF",
        ]))
        .unwrap();
        assert!(d.diagnostics.iter().any(|x| x.severity == Severity::Warning && x.message.contains("none placed")), "{:?}", d.diagnostics);
    }
}
