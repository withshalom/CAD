//! Round-trip, journal, crash-safety and corruption tests for the native
//! format. The sample drawing uses every `ObjectKind`.

use super::*;
use crate::geom::{Vec2, Vec3};
use std::panic::{catch_unwind, AssertUnwindSafe};

fn ent(space: Space, layer: u64) -> EntityCommon {
    EntityCommon {
        space,
        layer: Handle(layer),
        linetype: LinetypeRef::ByLayer,
        color: Color::ByLayer,
        transparency: None,
        linetype_scale: 1.0,
        lineweight: 0,
        invisible: false,
        plotstyle: Handle::NULL,
        material: Handle::NULL,
    }
}

fn obj(
    h: u64,
    name: &str,
    code: u16,
    owner: u64,
    e: Option<EntityCommon>,
    kind: ObjectKind,
) -> Object {
    Object {
        handle: Handle(h),
        type_code: code,
        type_name: name.into(),
        owner: Handle(owner),
        reactors: vec![],
        xdictionary: Handle::NULL,
        entity: e,
        kind,
        offset: 0,
        size: 0,
        note: None,
        raw: None,
        xdata: Vec::new(),
        unknown_tags: Vec::new(),
    }
}

fn v3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}
fn v2(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// A drawing that exercises every object kind and every optional field.
pub(super) fn sample() -> Drawing {
    use ObjectKind as K;
    let ms = 0x1F;
    let layer0 = 0x10;
    let m = || Some(ent(Space::Model, layer0));
    let mut objs = vec![
        obj(
            0x1,
            "BLOCK_CONTROL",
            0x30,
            0,
            None,
            K::Control(Control {
                entries: vec![Handle(0x40)],
                model_space: Handle(ms),
                paper_space: Handle(0x1E),
            }),
        ),
        obj(
            0x2,
            "LAYER_CONTROL",
            0x32,
            0,
            None,
            K::Control(Control {
                entries: vec![Handle(layer0), Handle(0x11)],
                ..Default::default()
            }),
        ),
        obj(
            layer0,
            "LAYER",
            0x33,
            0x2,
            None,
            K::Layer(Layer {
                name: "0".into(),
                flags: 0,
                frozen: false,
                off: false,
                locked: false,
                plot: true,
                color: Color::Index(7),
                linetype: Handle(0x15),
                lineweight: 0xFF,
            }),
        ),
        obj(
            0x11,
            "LAYER",
            0x33,
            0x2,
            None,
            K::Layer(Layer {
                name: "CUT-PLASMA — ø12".into(),
                flags: 0x44,
                frozen: true,
                off: true,
                locked: true,
                plot: false,
                color: Color::Rgb(255, 128, 0),
                linetype: Handle::NULL,
                lineweight: 25,
            }),
        ),
        obj(
            0x15,
            "LTYPE",
            0x39,
            0x5,
            None,
            K::NamedRecord(NamedRecord {
                name: "CONTINUOUS".into(),
            }),
        ),
        obj(
            ms,
            "BLOCK_HEADER",
            0x31,
            0x1,
            None,
            K::BlockHeader(BlockHeader {
                name: "*Model_Space".into(),
                entities: vec![Handle(0x100), Handle(0x101)],
                layout: Handle(0x22),
                insert_units: 4,
                ..Default::default()
            }),
        ),
        obj(
            0x1E,
            "BLOCK_HEADER",
            0x31,
            0x1,
            None,
            K::BlockHeader(BlockHeader {
                name: "*Paper_Space".into(),
                ..Default::default()
            }),
        ),
        obj(
            0x40,
            "BLOCK_HEADER",
            0x31,
            0x1,
            None,
            K::BlockHeader(BlockHeader {
                name: "FLANGE".into(),
                anonymous: false,
                has_attribs: true,
                is_xref: true,
                is_overlaid: true,
                base_point: v3(1.0, 2.0, 0.0),
                xref_path: "C:\\parts\\flange.dwg".into(),
                description: "6in flange".into(),
                insert_units: 1,
                block_entity: Handle(0x41),
                entities: vec![],
                first_entity: Handle(0x42),
                last_entity: Handle(0x43),
                endblk: Handle(0x44),
                layout: Handle::NULL,
                part: Part {
                    material: "Stainless steel".into(),
                    grade: "316L".into(),
                    thickness: 0.25,
                    quantity: 12,
                },
            }),
        ),
        obj(
            0x41,
            "BLOCK",
            0x04,
            0x40,
            Some(ent(Space::Owned, layer0)),
            K::BlockBegin {
                name: "FLANGE".into(),
            },
        ),
        obj(
            0x44,
            "ENDBLK",
            0x05,
            0x40,
            Some(ent(Space::Owned, layer0)),
            K::BlockEnd,
        ),
        obj(
            0x100,
            "LINE",
            0x13,
            0,
            m(),
            K::Line(Line {
                start: v3(0.0, 0.0, 0.0),
                end: v3(100.0, 50.0, 0.0),
                thickness: 0.0,
                extrusion: Vec3::Z,
            }),
        ),
        obj(
            0x101,
            "CIRCLE",
            0x12,
            0,
            m(),
            K::Circle(Circle {
                center: v3(10.0, 10.0, 0.0),
                radius: 6.35,
                thickness: 1.5,
                extrusion: Vec3::Z,
            }),
        ),
        obj(
            0x102,
            "ARC",
            0x11,
            0,
            m(),
            K::Arc(Arc {
                center: v3(5.0, 5.0, 0.0),
                radius: 2.0,
                thickness: 0.0,
                extrusion: v3(0.0, 0.0, -1.0),
                start_angle: 0.1,
                end_angle: std::f64::consts::PI,
            }),
        ),
        obj(
            0x103,
            "POINT",
            0x1B,
            0,
            m(),
            K::Point(Point {
                position: v3(1.0, 1.0, 1.0),
                thickness: 0.0,
                extrusion: Vec3::Z,
                x_axis_angle: 0.3,
            }),
        ),
        obj(
            0x104,
            "ELLIPSE",
            0x23,
            0,
            m(),
            K::Ellipse(Ellipse {
                center: v3(0.0, 0.0, 0.0),
                major_axis: v3(4.0, 0.0, 0.0),
                extrusion: Vec3::Z,
                axis_ratio: 0.5,
                start_param: 0.0,
                end_param: 6.0,
            }),
        ),
        obj(
            0x105,
            "LWPOLYLINE",
            0x4D,
            0,
            m(),
            K::LwPolyline(LwPolyline {
                flags: LwPolyline::FLAG_CLOSED,
                const_width: 0.0,
                elevation: 2.0,
                thickness: 0.0,
                extrusion: Vec3::Z,
                points: vec![v2(0.0, 0.0), v2(10.0, 0.0), v2(10.0, 5.0)],
                bulges: vec![0.0, 0.414, 0.0],
                widths: vec![(0.1, 0.2)],
                vertex_ids: vec![1, 2, 3],
            }),
        ),
        obj(
            0x106,
            "POLYLINE_2D",
            0x0F,
            0,
            m(),
            K::Polyline(Polyline {
                kind: PolylineKind::PolyfaceMesh,
                flags: 64,
                curve_type: 5,
                start_width: 0.5,
                end_width: 0.25,
                thickness: 0.0,
                elevation: 1.0,
                extrusion: Vec3::Z,
                vertices: vec![Handle(0x107)],
                first_vertex: Handle(0x107),
                last_vertex: Handle(0x107),
                seqend: Handle(0x108),
            }),
        ),
        obj(
            0x107,
            "VERTEX_PFACE_FACE",
            0x0E,
            0x106,
            Some(ent(Space::Owned, layer0)),
            K::Vertex(Vertex {
                kind: VertexKind::PfaceFace,
                flags: 128,
                point: v3(1.0, 2.0, 3.0),
                start_width: 0.0,
                end_width: 0.0,
                bulge: 0.25,
                tangent_dir: 0.0,
                id: 9,
                face: [1, -2, 3, 0],
            }),
        ),
        obj(
            0x108,
            "SEQEND",
            0x06,
            0x106,
            Some(ent(Space::Owned, layer0)),
            K::SeqEnd,
        ),
        obj(
            0x109,
            "TEXT",
            0x01,
            0,
            m(),
            K::Text(Text {
                insertion: v2(1.0, 2.0),
                alignment: v2(3.0, 4.0),
                elevation: 0.0,
                extrusion: Vec3::Z,
                thickness: 0.0,
                oblique: 0.2,
                rotation: 1.0,
                height: 2.5,
                width_factor: 0.8,
                value: "PART #12 — 3/8\" A36".into(),
                generation: 2,
                h_align: 1,
                v_align: 2,
                style: Handle(0x16),
            }),
        ),
        obj(
            0x10A,
            "ATTDEF",
            0x03,
            0x40,
            Some(ent(Space::Owned, layer0)),
            K::Attribute(Attribute {
                text: Text {
                    value: "QTY".into(),
                    height: 1.0,
                    ..Default::default()
                },
                tag: "QTY".into(),
                prompt: "Quantity?".into(),
                flags: 8,
                field_length: 3,
                is_definition: true,
            }),
        ),
        obj(
            0x10B,
            "MTEXT",
            0x2C,
            0,
            m(),
            K::MText(MText {
                insertion: v3(0.0, 20.0, 0.0),
                extrusion: Vec3::Z,
                x_axis: Vec3::X,
                rect_width: 40.0,
                rect_height: 10.0,
                text_height: 2.0,
                attachment: 1,
                drawing_dir: 5,
                contents: "\\fArial|b1;NOTES\\PDeburr all edges".into(),
                linespacing_factor: 1.0,
                style: Handle::NULL,
            }),
        ),
        obj(
            0x10C,
            "MINSERT",
            0x08,
            0,
            m(),
            K::Insert(Insert {
                insertion: v3(50.0, 50.0, 0.0),
                scale: v3(1.0, -1.0, 1.0),
                rotation: 0.5,
                extrusion: Vec3::Z,
                has_attribs: true,
                block_header: Handle(0x40),
                attribs: vec![Handle(0x10D)],
                first_attrib: Handle(0x10D),
                last_attrib: Handle(0x10D),
                seqend: Handle(0x10E),
                array: Some((3, 2, 10.0, 12.5)),
            }),
        ),
        obj(
            0x10F,
            "INSERT",
            0x07,
            0,
            m(),
            K::Insert(Insert {
                insertion: v3(0.0, 0.0, 0.0),
                scale: v3(2.0, 2.0, 2.0),
                block_header: Handle(0x40),
                ..Default::default()
            }),
        ),
        obj(
            0x110,
            "SOLID",
            0x1F,
            0,
            m(),
            K::Solid(Solid {
                thickness: 0.0,
                elevation: 0.0,
                corners: [v2(0.0, 0.0), v2(1.0, 0.0), v2(0.0, 1.0), v2(1.0, 1.0)],
                extrusion: Vec3::Z,
                is_trace: true,
            }),
        ),
        obj(
            0x111,
            "3DFACE",
            0x1C,
            0,
            m(),
            K::Face3d(Face3d {
                corners: [
                    v3(0.0, 0.0, 0.0),
                    v3(1.0, 0.0, 0.0),
                    v3(1.0, 1.0, 1.0),
                    v3(0.0, 1.0, 1.0),
                ],
                invisible_edges: 5,
            }),
        ),
        obj(
            0x112,
            "SPLINE",
            0x24,
            0,
            m(),
            K::Spline(Spline {
                degree: 3,
                rational: true,
                closed: false,
                periodic: true,
                knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                control_points: vec![
                    v3(0.0, 0.0, 0.0),
                    v3(1.0, 2.0, 0.0),
                    v3(3.0, 2.0, 0.0),
                    v3(4.0, 0.0, 0.0),
                ],
                weights: vec![1.0, 0.7, 0.7, 1.0],
                fit_points: vec![v3(0.0, 0.0, 0.0)],
                fit_tolerance: 1e-10,
                start_tangent: Vec3::X,
                end_tangent: Vec3::Y,
            }),
        ),
        obj(
            0x113,
            "DIMENSION_ALIGNED",
            0x16,
            0,
            m(),
            K::Dimension(Dimension {
                kind: DimensionKind::Aligned,
                extrusion: Vec3::Z,
                text_midpoint: v2(5.0, 5.0),
                elevation: 0.0,
                flags: 33,
                user_text: "<> TYP".into(),
                text_rotation: 0.0,
                horizontal_direction: 0.0,
                insert_scale: v3(1.0, 1.0, 1.0),
                insert_rotation: 0.0,
                attachment: 5,
                measurement: 100.0,
                clone_insertion: v2(0.0, 0.0),
                pt10: v3(0.0, 10.0, 0.0),
                pt13: v3(0.0, 0.0, 0.0),
                pt14: v3(100.0, 0.0, 0.0),
                pt15: Vec3::default(),
                pt16: Vec2::default(),
                ext_line_rotation: 0.0,
                dim_rotation: 0.0,
                leader_length: 0.0,
                dimstyle: Handle(0x27),
                block: Handle(0x28),
            }),
        ),
        obj(
            0x114,
            "HATCH",
            0x4E,
            0,
            m(),
            K::Hatch(Hatch {
                elevation: 0.0,
                extrusion: Vec3::Z,
                pattern_name: "ANSI31".into(),
                solid_fill: false,
                associative: true,
                is_gradient: false,
                style: 1,
                pattern_type: 1,
                pattern_angle: 0.785,
                pattern_scale: 2.0,
                pattern_double: false,
                pattern_lines: Vec::new(),
                seed_points: Vec::new(),
                paths: vec![
                    HatchPath::Polyline {
                        flags: 3,
                        closed: true,
                        vertices: vec![
                            (v2(0.0, 0.0), 0.0),
                            (v2(5.0, 0.0), 1.0),
                            (v2(5.0, 5.0), 0.0),
                        ],
                    },
                    HatchPath::Edges {
                        flags: 1,
                        edges: vec![
                            HatchEdge::Line {
                                a: v2(0.0, 0.0),
                                b: v2(1.0, 0.0),
                            },
                            HatchEdge::Arc {
                                center: v2(1.0, 1.0),
                                radius: 1.0,
                                start: 0.0,
                                end: 1.5,
                                ccw: true,
                            },
                            HatchEdge::Ellipse {
                                center: v2(0.0, 0.0),
                                major: v2(2.0, 0.0),
                                ratio: 0.5,
                                start: 0.0,
                                end: 3.0,
                                ccw: false,
                            },
                            HatchEdge::Spline {
                                degree: 3,
                                knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                                control_points: vec![
                                    v2(0.0, 0.0),
                                    v2(1.0, 1.0),
                                    v2(2.0, 1.0),
                                    v2(3.0, 0.0),
                                ],
                                weights: vec![],
                            },
                        ],
                    },
                ],
            }),
        ),
        obj(0x115, "ACAD_TABLE", 0x1F5, 0, m(), K::Unsupported),
        obj(
            0x116,
            "MLINE",
            0x2F,
            0,
            m(),
            K::Failed("bad bits at 0x1C".into()),
        ),
        obj(0x117, "DICTIONARY", 0x2A, 0xC, None, K::Unsupported),
    ];
    // Optional fields and edge values on a few objects.
    {
        let l = &mut objs[10];
        l.reactors = vec![Handle(0x200), Handle(0x201)];
        l.xdictionary = Handle(0x202);
        l.offset = 123_456;
        l.size = 42;
        l.note = Some("common handles: truncated".into());
        let e = l.entity.as_mut().unwrap();
        e.linetype = LinetypeRef::Handle(Handle(0x15));
        e.color = Color::Rgb(1, 2, 3);
        e.transparency = Some(0x0200_0033);
        e.lineweight = 50;
        e.invisible = true;
        e.plotstyle = Handle(0x30);
        e.material = Handle(0x31);
        e.linetype_scale = 2.5;
    }
    objs[11].entity.as_mut().unwrap().color = Color::ByBlock;
    objs[11].entity.as_mut().unwrap().linetype = LinetypeRef::Continuous;
    objs[12].entity.as_mut().unwrap().color = Color::None;
    objs[12].entity.as_mut().unwrap().linetype = LinetypeRef::ByBlock;
    objs[12].entity.as_mut().unwrap().space = Space::Paper;
    // Extended data with every value kind, on the LINE.
    objs[10].xdata = vec![
        XData {
            app: XDataApp::Name("ACAD".into()),
            items: vec![
                XValue::Str("DSTYLE".into()),
                XValue::Open,
                XValue::Int16(271),
                XValue::Int16(3),
                XValue::Close,
            ],
        },
        XData {
            app: XDataApp::Name("CAD_FAB".into()),
            items: vec![
                XValue::Str("cut — ø12".into()),
                XValue::Layer(Handle(0x11)),
                XValue::Bytes(vec![0, 1, 2, 254, 255]),
                XValue::Handle(Handle(0x101)),
                XValue::Point(v3(1.0, 2.0, 3.0)),
                XValue::Position(v3(4.0, 5.0, 6.0)),
                XValue::Displacement(v3(-1.0, 0.0, 0.5)),
                XValue::Direction(v3(0.0, 1.0, 0.0)),
                XValue::Real(0.06),
                XValue::Distance(1.5),
                XValue::ScaleFactor(2.0),
                XValue::Int16(-7),
                XValue::Int32(1_000_000),
            ],
        },
    ];
    // Fields a newer writer added, then extended data naming a missing
    // application record and holding DWG bytes that did not decode.
    {
        let mut w = TagWriter::new();
        w.f64(2998, 1.25);
        w.str(2999, "from a later revision");
        objs[11].unknown_tags = w.buf;
        objs[11].xdata = vec![XData {
            app: XDataApp::Handle(Handle(0x77)),
            items: vec![
                XValue::Str(String::new()),
                XValue::Undecoded {
                    version: DwgVersion::R2004,
                    data: vec![99, 1, 2, 3],
                },
            ],
        }];
    }
    for (h, name) in [(0x120, "ACAD"), (0x121, "CAD_FAB")] {
        objs.push(obj(
            h,
            "APPID",
            0x43,
            0,
            None,
            K::NamedRecord(NamedRecord { name: name.into() }),
        ));
    }
    // Format 1.1 table records: a linetype with a plain and a complex
    // element, a text style and a dimension style.
    objs.push(obj(
        0x122,
        "LTYPE",
        0x39,
        0x5,
        None,
        K::Linetype(Linetype {
            name: "GAS_LINE".into(),
            description: "Gas line ----GAS----".into(),
            flags: 64,
            pattern_length: 15.0,
            elements: vec![
                LinetypeElement::plain(12.7),
                LinetypeElement {
                    length: -2.3,
                    complex: 2,
                    shape: 0,
                    style: Handle(0x123),
                    scale: 0.1,
                    rotation: 0.25,
                    offset: v2(-0.1, -0.05),
                    text: "GAS".into(),
                },
            ],
        }),
    ));
    objs.push(obj(
        0x123,
        "STYLE",
        0x35,
        0x3,
        None,
        K::TextStyle(TextStyle {
            name: "Notes".into(),
            flags: 4,
            height: 3.5,
            width_factor: 0.8,
            oblique: 0.26,
            generation: 2,
            last_height: 2.5,
            font_file: "simplex.shx".into(),
            bigfont_file: "bigfont.shx".into(),
        }),
    ));
    objs.push(obj(
        0x124,
        "DIMSTYLE",
        0x45,
        0xA,
        None,
        K::DimStyle(DimStyle {
            name: "Mech — mm".into(),
            flags: 0,
            post_text: "<> TYP".into(),
            scale: 2.0,
            arrow_size: 3.0,
            ext_offset: 1.5,
            baseline_spacing: 10.0,
            ext_extension: 2.0,
            rounding: 0.5,
            dim_line_extension: 1.25,
            text_height: 3.5,
            center_mark: -2.5,
            tick_size: 0.0,
            alt_factor: 0.039_370_078_740_157_48,
            linear_factor: 0.5,
            text_gap: 0.625,
            text_inside_horizontal: false,
            text_outside_horizontal: true,
            text_above: 1,
            zero_suppression: 8,
            alt_units: true,
            alt_decimals: 3,
            line_color: Color::Index(3),
            ext_color: Color::Rgb(10, 20, 30),
            text_color: Color::ByBlock,
            decimals: 2,
            fraction_format: 2,
            unit_format: 4,
            decimal_separator: ',',
            text_justify: 3,
            text_style: Handle(0x123),
            line_weight: 7,
            ext_weight: Lineweight::CODE_BY_LAYER,
        }),
    ));
    // An object of a kind from a newer revision, with its fields and
    // extended data.
    {
        let mut future = obj(0x118, "FUTURE_THING", 600, 0, None, K::NewerKind(99));
        let mut w = TagWriter::new();
        w.p3(10, Vec3::X);
        w.str(2999, "later");
        future.unknown_tags = w.buf;
        future.xdata = vec![XData {
            app: XDataApp::Name("CAD_FAB".into()),
            items: vec![XValue::Int16(1)],
        }];
        objs.push(future);
    }
    for o in objs.iter_mut() {
        if matches!(o.kind, ObjectKind::Unsupported | ObjectKind::Failed(_)) {
            o.raw = Some(RawDwgObject {
                version: DwgVersion::R2018,
                handle_stream_bits: 77,
                data: (0..=255u8).cycle().take(300).collect(),
            });
        }
    }

    let mut d = Drawing {
        version: DwgVersion::R2018,
        maintenance_version: 4,
        codepage: 30,
        header: HeaderVars {
            verified: true,
            insunits: Some(1),
            measurement: Some(0),
            extmin: Some(v3(-1.0, -2.0, 0.0)),
            extmax: Some(v3(100.0, 60.0, 0.0)),
            limmin: Some(v2(0.0, 0.0)),
            limmax: Some(v2(12.0, 9.0)),
            insbase: Some(Vec3::default()),
            ltscale: Some(1.0),
            textsize: Some(0.2),
            lunits: Some(4),
            luprec: Some(6),
            aunits: Some(1),
            auprec: Some(2),
            angbase: Some(0.5),
            angdir: Some(1),
            celtscale: Some(0.5),
            handseed: Some(0x300),
            clayer: Handle(layer0),
            textstyle: Handle(0x123),
            celtype: Handle(0x15),
            dimstyle: Handle(0x124),
            block_control: Handle(0x1),
            layer_control: Handle(0x2),
            style_control: Handle(0x3),
            linetype_control: Handle(0x5),
            dimstyle_control: Handle(0xA),
            model_space: Handle(ms),
            paper_space: Handle(0x1E),
            named_objects_dict: Handle(0xC),
            raw: vec![
                ("$ACADVER".into(), "AC1032".into()),
                ("$DIMSCALE".into(), "1.0".into()),
            ],
            note: String::new(),
            properties: Properties {
                title: "Bracket, left".into(),
                subject: "Sample".into(),
                author: "Drafting".into(),
                keywords: "plate; 6 mm".into(),
                comments: "Line one\nline two".into(),
                last_saved_by: "cad-core".into(),
                revision: "C".into(),
                hyperlink_base: "https://example.com/parts/".into(),
                created: 1_700_000_000,
                modified: 1_700_086_400,
                custom: vec![
                    ("Material".into(), "S355".into()),
                    ("Client".into(), "Ünïcödé Ltd".into()),
                ],
            },
            unknown_tags: {
                let mut w = TagWriter::new();
                w.str(9, "$CAD_FROM_LATER_REVISION");
                w.f64(40, 2.0);
                w.buf
            },
        },
        classes: vec![ClassDef {
            number: 500,
            proxy_flags: 1152,
            app_name: "ObjectDBX Classes".into(),
            cpp_name: "AcDbTable".into(),
            dxf_name: "ACAD_TABLE".into(),
            was_zombie: false,
            item_class_id: 0x1F2,
            instance_count: 3,
            dwg_version: 27,
            maintenance_version: 1,
        }],
        objects: objs.into_iter().map(|o| (o.handle, o)).collect(),
        sections: vec![],
        diagnostics: vec![],
        index: DrawingIndex::default(),
    };
    d.rebuild_index();
    d
}

fn assert_same(a: &Drawing, b: &Drawing) {
    assert_eq!(a.version, b.version);
    assert_eq!(a.maintenance_version, b.maintenance_version);
    assert_eq!(a.codepage, b.codepage);
    assert_eq!(a.header, b.header);
    assert_eq!(a.classes, b.classes);
    assert_eq!(a.objects.len(), b.objects.len());
    for (h, o) in &a.objects {
        assert_eq!(Some(o), b.objects.get(h), "object {h}");
    }
    assert_eq!(a.index, b.index);
}

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cad-native-{tag}-{}-{}",
        std::process::id(),
        now_unix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("t.cad")
}

#[test]
fn roundtrip_every_kind() {
    let d = sample();
    for compress in [true, false] {
        let opts = WriteOptions {
            compress,
            ..Default::default()
        };
        let bytes = to_bytes(&d, &opts);
        let back = read_bytes(&bytes).unwrap();
        assert_same(&d, &back);
        assert!(
            back.diagnostics
                .iter()
                .all(|x| x.severity == Severity::Info),
            "{:?}",
            back.diagnostics
        );
        // Writing the result again gives identical object data.
        let again = to_bytes(&back, &opts);
        assert_same(&back, &read_bytes(&again).unwrap());
    }
}

#[test]
fn open_detects_format_and_preview_exists() {
    let d = sample();
    let bytes = to_bytes(&d, &WriteOptions::default());
    assert!(is_native(&bytes));
    assert_same(&d, &crate::open_bytes(&bytes).unwrap());
    let svg = preview_svg(&bytes).unwrap().expect("preview");
    assert!(
        svg.starts_with("<svg") || svg.starts_with("<?xml"),
        "{}",
        &svg[..40]
    );
    let info = inspect(&bytes).unwrap();
    assert_eq!(info.format, (FORMAT_MAJOR, FORMAT_MINOR));
    assert_eq!(info.source_version, Some(DwgVersion::R2018));
    assert!(info.sections.iter().all(|s| s.crc_ok));
    assert!(info.sections.iter().any(|s| s.name == "DwgPreserved"));
}

#[test]
fn many_objects_use_several_pages_and_random_access() {
    let mut d = sample();
    let ms = d.model_space();
    for i in 0..20_000u64 {
        let h = 0x10_000 + i;
        d.objects.insert(
            Handle(h),
            obj(
                h,
                "LINE",
                0x13,
                0,
                Some(ent(Space::Model, 0x10)),
                ObjectKind::Line(Line {
                    start: v3(i as f64, 0.0, 0.0),
                    end: v3(i as f64, 10.0, 0.0),
                    thickness: 0.0,
                    extrusion: Vec3::Z,
                }),
            ),
        );
    }
    let _ = ms;
    d.rebuild_index();
    let bytes = to_bytes(
        &d,
        &WriteOptions {
            preview: false,
            ..Default::default()
        },
    );
    let info = inspect(&bytes).unwrap();
    let pages = info
        .sections
        .iter()
        .filter(|s| s.name.starts_with("Objects."))
        .count();
    assert!(pages > 5, "{pages} pages");
    let total_raw: u64 = info.sections.iter().map(|s| s.raw).sum();
    assert!(
        (bytes.len() as u64) < total_raw / 2,
        "compression should at least halve repetitive data"
    );
    let back = read_bytes(&bytes).unwrap();
    assert_same(&d, &back);
    for h in [0x100u64, 0x10_000, 0x10_000 + 19_999, 0x14_E20] {
        let one = read_object(&bytes, Handle(h)).unwrap();
        assert_eq!(
            one.as_ref(),
            d.objects
                .get(&Handle(h))
                .map(|o| {
                    let mut o = o.clone();
                    o.raw = None; // random access does not attach preserved bytes
                    o
                })
                .as_ref()
        );
    }
    assert_eq!(read_object(&bytes, Handle(0xDEAD_BEEF)).unwrap(), None);
}

#[test]
fn journal_append_reload_and_compact() {
    let path = temp("journal");
    let d = sample();
    assert_eq!(write_file(&d, &path, &WriteOptions::default()).unwrap(), 1);

    let mut moved = d.objects[&Handle(0x100)].clone();
    if let ObjectKind::Line(l) = &mut moved.kind {
        l.end = v3(200.0, 0.0, 0.0);
    }
    let added = obj(
        0x400,
        "CIRCLE",
        0x12,
        0,
        Some(ent(Space::Model, 0x10)),
        ObjectKind::Circle(Circle {
            center: v3(1.0, 1.0, 0.0),
            radius: 3.0,
            thickness: 0.0,
            extrusion: Vec3::Z,
        }),
    );
    let set = ChangeSet {
        author: "shalom".into(),
        time: 1_790_000_000,
        changes: vec![
            Change::Put(Box::new(moved.clone())),
            Change::Put(Box::new(added.clone())),
            Change::Delete(Handle(0x101)),
        ],
    };
    assert_eq!(append_changes(&path, &set).unwrap(), 2);
    let mut h2 = d.header.clone();
    h2.insunits = Some(4);
    let set2 = ChangeSet {
        author: "shalom".into(),
        time: 0,
        changes: vec![
            Change::Header(Box::new(h2.clone())),
            Change::Delete(Handle(0x400)),
        ],
    };
    assert_eq!(append_changes(&path, &set2).unwrap(), 3);

    let mut expected = d.clone();
    apply(&mut expected, &set);
    apply(&mut expected, &set2);
    let back = read_file(&path).unwrap();
    assert_same(&expected, &back);
    assert!(back.get(Handle(0x101)).is_none());
    assert_eq!(back.header.insunits, Some(4));
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(inspect(&bytes).unwrap().change_sets, 2);
    assert_eq!(
        read_object(&bytes, Handle(0x100)).unwrap().unwrap().kind,
        moved.kind
    );
    assert_eq!(read_object(&bytes, Handle(0x400)).unwrap(), None);

    // A full save merges the journal.
    assert_eq!(
        write_file(&back, &path, &WriteOptions::default()).unwrap(),
        4
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(inspect(&bytes).unwrap().change_sets, 0);
    assert_same(&expected, &read_bytes(&bytes).unwrap());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn interrupted_saves_leave_a_readable_file() {
    let path = temp("crash");
    let d = sample();
    write_file(&d, &path, &WriteOptions::default()).unwrap();
    let before = std::fs::read(&path).unwrap();
    let set = ChangeSet {
        author: "a".into(),
        time: 1,
        changes: vec![Change::Delete(Handle(0x100))],
    };
    append_changes(&path, &set).unwrap();
    let after = std::fs::read(&path).unwrap();
    let hl = container::HEADER_LEN;

    // 1. Crash before anything reached the disk except part of the journal:
    //    old header, junk at the end.
    for cut in [before.len() + 3, after.len() - hl - 5, after.len() - 1] {
        let mut crashed = after[..cut].to_vec();
        crashed[..hl].copy_from_slice(&before[..hl]);
        let r = read_bytes(&crashed).unwrap();
        assert!(
            r.get(Handle(0x100)).is_some(),
            "cut {cut}: old state expected"
        );
    }
    // 2. Crash after the trailer was written, before the front header:
    //    the trailer's newer generation wins.
    let mut crashed = after.clone();
    crashed[..hl].copy_from_slice(&before[..hl]);
    let r = read_bytes(&crashed).unwrap();
    assert!(r.get(Handle(0x100)).is_none(), "new state expected");
    // 3. Torn front header: the trailer copy is used.
    let mut torn = after.clone();
    torn[20] ^= 0xFF;
    let r = read_bytes(&torn).unwrap();
    assert!(r.get(Handle(0x100)).is_none());
    assert!(r.diagnostics.iter().any(|x| x.message.contains("damaged")));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn damaged_files_never_panic() {
    let d = sample();
    let good = to_bytes(&d, &WriteOptions::default());
    let raw = to_bytes(
        &d,
        &WriteOptions {
            compress: false,
            ..Default::default()
        },
    );
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for base in [&good, &raw] {
        for cut in (0..base.len()).step_by(7) {
            let r = catch_unwind(AssertUnwindSafe(|| {
                let _ = read_bytes(&base[..cut]);
                let _ = inspect(&base[..cut]);
                let _ = audit::audit(&base[..cut]);
            }));
            assert!(r.is_ok(), "panic on truncation at {cut}");
        }
        for _ in 0..3000 {
            let mut b = base.to_vec();
            for _ in 0..(1 + next() % 4) {
                let i = (next() as usize) % b.len();
                b[i] ^= 1 << (next() % 8);
            }
            let r = catch_unwind(AssertUnwindSafe(|| {
                let _ = read_bytes(&b);
                let _ = read_object(&b, Handle(0x100));
                let _ = preview_svg(&b);
                let _ = audit::audit(&b);
            }));
            assert!(r.is_ok(), "panic on bit flip");
        }
    }
}

#[test]
fn damage_inside_a_section_is_reported_not_fatal() {
    let d = sample();
    let mut b = to_bytes(
        &d,
        &WriteOptions {
            compress: false,
            preview: false,
            recovery: false,
            ..Default::default()
        },
    );
    let c = Container::parse(&b).unwrap();
    let page = c.entry("Objects.0").unwrap().clone();
    // Flip one byte in the middle of the page (inside some record's value).
    let at = (page.offset + page.stored_len / 2) as usize;
    b[at] ^= 0x01;
    let r = read_bytes(&b).unwrap();
    assert!(r
        .diagnostics
        .iter()
        .any(|x| x.message.contains("CRC mismatch")));
    assert!(r.objects.len() >= d.objects.len() - 1);
}

#[test]
fn newer_writers_fields_are_kept() {
    // What a later revision might write for an object: the fields this
    // version knows, two it added, then extended data.
    let d = sample();
    let o = d.objects[&Handle(0x105)].clone();
    assert!(o.xdata.is_empty() && o.unknown_tags.is_empty() && o.raw.is_none());
    let mut w = TagWriter::new();
    rec::write_object(&mut w, o.handle, &o);
    let mut added = TagWriter::new();
    added.f64(2999, 1.0);
    added.str(2998, "future");
    w.encoded(&added.buf);
    let x = XData {
        app: XDataApp::Name("APP".into()),
        items: vec![XValue::Int32(5)],
    };
    rec::write_xdata(&mut w, &x);
    let (_, back, problem) = rec::read_object_checked(&w.buf).unwrap();
    assert_eq!(problem, None);
    assert_eq!(
        back,
        Object {
            unknown_tags: added.buf.clone(),
            xdata: vec![x],
            ..o
        }
    );
    // Writing it again reproduces the record exactly.
    let mut again = TagWriter::new();
    rec::write_object(&mut again, back.handle, &back);
    assert_eq!(again.buf, w.buf);

    // Header variables this version does not know survive too; they are
    // written after the known ones.
    let mut future = TagWriter::new();
    future.str(9, "$SOMEFUTUREVAR");
    future.p3(10, Vec3::X);
    let mut known = d.header.clone();
    known.unknown_tags.clear();
    let mut w = TagWriter::new();
    w.encoded(&future.buf);
    rec::write_header(&mut w, &known);
    let first = rec::read_header(&w.buf).unwrap();
    assert_eq!(first.unknown_tags, future.buf);
    assert_eq!(
        HeaderVars {
            unknown_tags: Vec::new(),
            ..first.clone()
        },
        known
    );
    let mut w2 = TagWriter::new();
    rec::write_header(&mut w2, &first);
    assert_eq!(rec::read_header(&w2.buf).unwrap(), first);
}

#[test]
fn tags_of_a_newer_value_type_are_stepped_over_and_kept() {
    // Group 2999 with a value type from a later revision (0x7E): a varint
    // length, then that many bytes.
    let newer = [0xB7u8, 0x0B, 0x7E, 3, 1, 2, 3];
    // INSERT ends with optional fields; the newer tag right after them does
    // not fail the object, and the extended data after it still decodes.
    let d = sample();
    let o = d
        .objects
        .values()
        .find(|o| matches!(o.kind, ObjectKind::Insert(_)))
        .unwrap()
        .clone();
    let x = XData {
        app: XDataApp::Name("APP".into()),
        items: vec![XValue::Int16(9)],
    };
    let mut w = TagWriter::new();
    rec::write_object(&mut w, o.handle, &o);
    w.encoded(&newer);
    rec::write_xdata(&mut w, &x);
    let (_, back, problem) = rec::read_object_checked(&w.buf).unwrap();
    assert_eq!(problem, None);
    assert_eq!(back.kind, o.kind);
    assert_eq!(back.unknown_tags, newer);
    assert_eq!(back.xdata, vec![x]);
    let mut again = TagWriter::new();
    rec::write_object(&mut again, back.handle, &back);
    assert_eq!(again.buf, w.buf);

    // In the header, a variable of a newer type does not hide the known
    // ones after it, so editing those cannot leave a stale copy behind.
    let mut h = HeaderVars {
        verified: true,
        ..Default::default()
    };
    let mut w = TagWriter::new();
    w.str(9, "$NEWTYPE");
    w.encoded(&newer);
    let unknown = w.buf.clone();
    w.str(9, "$CLAYER");
    w.handle(330, Handle(0x10));
    let (read, problem) = rec::read_header_checked(&w.buf).unwrap();
    assert_eq!(problem, None);
    assert_eq!(read.clayer, Handle(0x10));
    assert_eq!(read.unknown_tags, unknown);
    h.clayer = Handle(0x20);
    h.unknown_tags = read.unknown_tags.clone();
    h.verified = read.verified;
    let mut w = TagWriter::new();
    rec::write_header(&mut w, &h);
    assert_eq!(rec::read_header(&w.buf).unwrap().clayer, Handle(0x20));
}

#[test]
fn unparseable_tags_are_kept_and_reported() {
    let d = sample();
    let o = d.objects[&Handle(0x100)].clone();
    let mut junk_o = o.clone();
    // Group 2999 with value type 0, which is never used: damage.
    junk_o.unknown_tags = vec![0xB7, 0x0B, 0x00];
    let mut w = TagWriter::new();
    rec::write_object(&mut w, junk_o.handle, &junk_o);
    let (_, back, problem) = rec::read_object_checked(&w.buf).unwrap();
    assert!(problem.unwrap().contains("kept unread"));
    assert!(
        back.xdata.is_empty(),
        "extended data after junk stays unread"
    );
    let mut again = TagWriter::new();
    rec::write_object(&mut again, back.handle, &back);
    assert_eq!(again.buf, w.buf);
    // A header stream cut short: the rest is kept and reported.
    let mut w = TagWriter::new();
    rec::write_header(&mut w, &d.header);
    w.encoded(&[9, 0]);
    let (_, problem) = rec::read_header_checked(&w.buf).unwrap();
    assert!(problem.is_some());
}

#[test]
fn malformed_extended_data_is_kept_whole_and_reported() {
    let d = sample();
    let o = d.objects[&Handle(0x103)].clone();
    assert!(o.xdata.is_empty());
    let mut w = TagWriter::new();
    rec::write_object(&mut w, o.handle, &o);
    let at = w.buf.len();
    w.str(1001, "APP");
    w.str(1000, "fine");
    w.str(1002, "("); // not a brace
    w.i16(1070, 4);
    let (_, back, problem) = rec::read_object_checked(&w.buf).unwrap();
    assert!(problem.unwrap().contains("1002"));
    assert!(back.xdata.is_empty());
    assert_eq!(back.unknown_tags, w.buf[at..].to_vec());
    assert_eq!(back.kind, o.kind);
    let mut again = TagWriter::new();
    rec::write_object(&mut again, back.handle, &back);
    assert_eq!(again.buf, w.buf);

    // Other malformed extended data: wrong value types, a stray code, a
    // version tag that is not exactly a known one.
    let bads: [fn(&mut TagWriter); 5] = [
        |w: &mut TagWriter| w.i16(1001, 3),
        |w: &mut TagWriter| {
            w.str(1001, "APP");
            w.str(1040, "1.5");
        },
        |w: &mut TagWriter| {
            w.str(1001, "APP");
            w.f64(40, 1.5);
        },
        |w: &mut TagWriter| {
            w.str(1001, "APP");
            w.str(rec::code::XDATA_RAW_VERSION, "AC9999");
            w.bytes(rec::code::XDATA_RAW, &[1]);
        },
        |w: &mut TagWriter| {
            w.str(1001, "APP");
            w.str(rec::code::XDATA_RAW_VERSION, "AC1018-not-a-version");
            w.bytes(rec::code::XDATA_RAW, &[1]);
        },
    ];
    for bad in bads {
        let mut w = TagWriter::new();
        rec::write_object(&mut w, o.handle, &o);
        bad(&mut w);
        let (_, back, problem) = rec::read_object_checked(&w.buf).unwrap();
        assert!(problem.is_some() && back.xdata.is_empty());
        let mut again = TagWriter::new();
        rec::write_object(&mut again, back.handle, &back);
        assert_eq!(again.buf, w.buf);
    }

    // In a file: the reader warns and the audit fails the record.
    let mut d2 = d.clone();
    d2.objects.insert(back.handle, back.clone());
    let bytes = to_bytes(&d2, &WriteOptions::default());
    let r = read_bytes(&bytes).unwrap();
    assert!(r
        .diagnostics
        .iter()
        .any(|x| x.severity == Severity::Warning && x.message.contains("partly decoded")));
    let rep = audit::audit(&bytes);
    assert!(rep.failed().any(|x| x.name == "object records"));

    // In a change set: the reader warns too.
    let path = temp("bad-xdata-journal");
    write_file(&d, &path, &WriteOptions::default()).unwrap();
    let set = ChangeSet {
        author: "a".into(),
        time: 1,
        changes: vec![Change::Put(Box::new(back))],
    };
    append_changes(&path, &set).unwrap();
    let r = read_file(&path).unwrap();
    assert!(r.diagnostics.iter().any(|x| x
        .message
        .contains("change set 2: 1 records only partly decoded")));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_kind_whose_fields_fail_is_written_back_as_found() {
    // A LINE record missing fields, followed by extended data.
    let d = sample();
    let line = d.objects[&Handle(0x100)].clone();
    assert!(!line.xdata.is_empty());
    let mut common = line.clone();
    common.kind = ObjectKind::Unsupported;
    common.xdata.clear();
    let mut w = TagWriter::new();
    rec::write_object(&mut w, common.handle, &common);
    w.buf.truncate(w.buf.len() - 5); // drop "2001 = 0"
    let mut body = TagWriter::new();
    body.i16(rec::code::KIND, 2); // LINE …
    body.p3(10, Vec3::X); // … with only its first field
    w.encoded(&body.buf);
    for x in &line.xdata {
        rec::write_xdata(&mut w, x);
    }

    let (_, back, problem) = rec::read_object_checked(&w.buf).unwrap();
    assert!(problem.is_some());
    assert!(matches!(back.kind, ObjectKind::Damaged(_)));
    assert_eq!(back.xdata, line.xdata);
    // The fields that failed are kept, starting with their kind tag …
    assert_eq!(back.unknown_tags, body.buf);
    // … and written back exactly as found, so the next reader reports the
    // same problem instead of seeing a settled "failed" object.
    let mut again = TagWriter::new();
    rec::write_object(&mut again, back.handle, &back);
    assert_eq!(again.buf, w.buf);
    let (_, back2, problem2) = rec::read_object_checked(&again.buf).unwrap();
    assert_eq!(problem2, problem);
    assert_eq!(back2, back);
    let mut d2 = d.clone();
    d2.objects.insert(back.handle, back);
    let bytes = to_bytes(&d2, &WriteOptions::default());
    assert!(audit::audit(&bytes)
        .failed()
        .any(|x| x.name == "object records"));
}

#[test]
fn records_without_a_kind_and_damaged_markers_stay_as_they_are() {
    let d = sample();
    let line = d.objects[&Handle(0x100)].clone();
    let mut common = line.clone();
    common.kind = ObjectKind::Unsupported;
    common.xdata.clear();
    let mut w = TagWriter::new();
    rec::write_object(&mut w, common.handle, &common);
    w.buf.truncate(w.buf.len() - 5); // drop "2001 = 0": no kind tag at all
    let head = w.buf.clone();
    // Common part only; common part, a newer field and extended data.
    let mut with_tail = TagWriter::new();
    with_tail.encoded(&head);
    with_tail.f64(2999, 1.0);
    for x in &line.xdata {
        rec::write_xdata(&mut with_tail, x);
    }
    for record in [head.clone(), with_tail.buf] {
        let (_, back, problem) = rec::read_object_checked(&record).unwrap();
        assert!(problem.is_some());
        assert!(matches!(back.kind, ObjectKind::Damaged(_)));
        let mut again = TagWriter::new();
        rec::write_object(&mut again, back.handle, &back);
        assert_eq!(again.buf, record, "written back as found");
        let (_, back2, problem2) = rec::read_object_checked(&again.buf).unwrap();
        assert_eq!((back2, problem2), (back, problem));
    }

    // A kind-1 object from DWG whose newer fields start with a damaged tag
    // coded 2001 keeps its marker and message.
    let failed = d.objects[&Handle(0x116)].clone();
    assert!(matches!(failed.kind, ObjectKind::Failed(_)));
    let mut w = TagWriter::new();
    rec::write_object(&mut w, failed.handle, &failed);
    w.encoded(&[0xD1, 0x07, 0x00]);
    let (_, back, problem) = rec::read_object_checked(&w.buf).unwrap();
    assert!(problem.is_some());
    assert_eq!(back.kind, failed.kind);
    let mut again = TagWriter::new();
    rec::write_object(&mut again, back.handle, &back);
    assert_eq!(again.buf, w.buf);
}

#[test]
fn a_known_header_variable_that_does_not_decode_is_kept() {
    let d = sample();
    let mut known = d.header.clone();
    known.unknown_tags.clear();
    // A value of the wrong type in the middle: the rest still loads.
    let mut w = TagWriter::new();
    w.str(9, "$TEXTSIZE");
    w.i16(40, 3);
    let bad = w.buf.clone();
    rec::write_header(
        &mut w,
        &HeaderVars {
            textsize: None,
            ..known.clone()
        },
    );
    let (h, problem) = rec::read_header_checked(&w.buf).unwrap();
    assert!(problem.unwrap().contains("$TEXTSIZE"));
    assert_eq!(h.textsize, None);
    assert_eq!(h.handseed, known.handseed);
    assert_eq!(h.unknown_tags, bad);
    let mut w2 = TagWriter::new();
    rec::write_header(&mut w2, &h);
    let (h2, problem2) = rec::read_header_checked(&w2.buf).unwrap();
    assert!(problem2.is_some());
    assert_eq!(h2, h);
    // A value that cannot be parsed at the end: what came before stays.
    let mut w = TagWriter::new();
    rec::write_header(&mut w, &known);
    let at = w.buf.len();
    w.str(9, "$TEXTSIZE");
    w.encoded(&[0x28, 0x00, 0x00]);
    let (h, problem) = rec::read_header_checked(&w.buf).unwrap();
    assert!(problem.is_some());
    assert_eq!(h.handseed, known.handseed);
    assert_eq!(h.model_space, known.model_space);
    assert_eq!(h.unknown_tags, w.buf[at..].to_vec());
}

#[test]
fn the_audit_reports_partly_decoded_change_sets() {
    let path = temp("audit-partial-journal");
    let d = sample();
    write_file(&d, &path, &WriteOptions::default()).unwrap();
    let mut o = d.objects[&Handle(0x103)].clone();
    let mut w = TagWriter::new();
    w.str(1001, "APP");
    w.str(1002, "("); // not a brace
    o.unknown_tags = w.buf;
    let set = ChangeSet {
        author: "a".into(),
        time: 1,
        changes: vec![Change::Put(Box::new(o))],
    };
    append_changes(&path, &set).unwrap();
    let rep = audit::audit(&std::fs::read(&path).unwrap());
    assert!(
        rep.failed()
            .any(|x| x.name == "change sets" && x.detail.contains("partly decoded")),
        "{:?}",
        rep.checks
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn extended_data_uses_dxf_group_codes() {
    let d = sample();
    let o = &d.objects[&Handle(0x101)];
    let mut w = TagWriter::new();
    rec::write_object(&mut w, o.handle, o);
    let mut p = 0;
    let mut tail = Vec::new();
    let mut seen_kind = false;
    while p < w.buf.len() {
        let t = tags::read_tag(&w.buf, &mut p).unwrap();
        if t.code == rec::code::KIND {
            seen_kind = true;
            continue;
        }
        if seen_kind && (t.code >= 1000 || t.code == 2998 || t.code == 2999) {
            tail.push((t.code, t.value.type_name()));
        }
    }
    assert_eq!(
        tail,
        vec![
            (2998, "double"),
            (2999, "string"),
            (1001, "handle"), // application record missing: kept by handle
            (1000, "string"),
            (2600, "string"),
            (2601, "bytes"),
        ]
    );
    let o = &d.objects[&Handle(0x100)];
    let mut w = TagWriter::new();
    rec::write_object(&mut w, o.handle, o);
    let mut p = 0;
    let mut codes = Vec::new();
    while p < w.buf.len() {
        let t = tags::read_tag(&w.buf, &mut p).unwrap();
        if (1000..=1071).contains(&t.code) {
            codes.push(t.code);
        }
    }
    assert_eq!(
        codes,
        vec![
            1001, 1000, 1002, 1070, 1070, 1002, // ACAD
            1001, 1000, 1003, 1004, 1005, 1010, 1011, 1012, 1013, 1040, 1041, 1042, 1070, 1071,
        ]
    );
}

#[test]
fn extended_data_and_newer_fields_travel_in_change_sets() {
    let path = temp("xdata-journal");
    let d = sample();
    write_file(&d, &path, &WriteOptions::default()).unwrap();
    let mut o = d.objects[&Handle(0x102)].clone();
    o.xdata = vec![XData {
        app: XDataApp::Name("CAD_FAB".into()),
        items: vec![XValue::Str("etch".into()), XValue::Distance(0.2)],
    }];
    let mut w = TagWriter::new();
    w.bool(2997, true);
    o.unknown_tags = w.buf;
    let set = ChangeSet {
        author: "a".into(),
        time: 1,
        changes: vec![Change::Put(Box::new(o.clone()))],
    };
    append_changes(&path, &set).unwrap();
    let back = read_file(&path).unwrap();
    assert_eq!(back.objects[&Handle(0x102)], o);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(read_object(&bytes, Handle(0x102)).unwrap().unwrap(), o);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn empty_drawing_roundtrips() {
    let mut d = sample();
    d.objects.clear();
    d.classes.clear();
    d.header = HeaderVars::default();
    d.rebuild_index();
    let b = to_bytes(&d, &WriteOptions::default());
    assert_same(&d, &read_bytes(&b).unwrap());
}

#[test]
fn a_dimension_style_coloured_index_0_reopens_as_by_block() {
    // ACI 0 is "by block": saved as that, not as a word the reader refuses.
    let mut d = crate::standards::new_drawing(crate::standards::Template::Metric);
    let h = d.header.dimstyle;
    match &mut d.objects.get_mut(&h).unwrap().kind {
        ObjectKind::DimStyle(s) => s.line_color = Color::Index(0),
        other => panic!("{other:?}"),
    }
    let back = read_bytes(&to_bytes(&d, &WriteOptions::default())).unwrap();
    match &back.objects[&h].kind {
        ObjectKind::DimStyle(s) => assert_eq!(s.line_color, Color::ByBlock),
        other => panic!("{other:?}"),
    }
    assert!(
        back.diagnostics
            .iter()
            .all(|d| d.severity == Severity::Info),
        "{:?}",
        back.diagnostics
    );
}

#[test]
fn a_1_0_file_of_an_r14_drawing_reads_lineweight_0_as_unset() {
    let mut d = crate::standards::new_drawing(crate::standards::Template::Metric);
    d.version = DwgVersion::R14;
    let zero = d.layer_by_name("0").unwrap().0;
    let line = d
        .add_entity(
            zero,
            ObjectKind::Line(Line::new(Vec2::ZERO, Vec2::new(1.0, 0.0))),
        )
        .unwrap();
    // What 1.0 stored for such a drawing: zeros everywhere.
    for o in d.objects.values_mut() {
        if let Some(e) = o.entity.as_mut() {
            e.lineweight = 0;
        }
        if let ObjectKind::Layer(l) = &mut o.kind {
            l.lineweight = 0;
        }
    }
    let as_1_1 = as_version(&to_bytes(&d, &WriteOptions::default()), 1, 1);
    let as_1_0 = as_version(&as_1_1, 1, 0);
    let weight_of = |d: &Drawing| d.objects[&line].entity.as_ref().unwrap().lineweight;
    let layer_weight = |d: &Drawing| match &d.objects[&zero].kind {
        ObjectKind::Layer(l) => l.lineweight,
        other => panic!("{other:?}"),
    };
    let back = read_bytes(&as_1_0).unwrap();
    assert_eq!(weight_of(&back), Lineweight::CODE_BY_LAYER);
    assert_eq!(layer_weight(&back), Lineweight::CODE_DEFAULT);
    let one = read_object(&as_1_0, line).unwrap().unwrap();
    assert_eq!(one.entity.unwrap().lineweight, Lineweight::CODE_BY_LAYER);
    // Written as 1.1, a zero is an explicit 0.00 mm and stays.
    let back = read_bytes(&as_1_1).unwrap();
    assert_eq!((weight_of(&back), layer_weight(&back)), (0, 0));
}

#[test]
fn mismatched_handles_keep_their_keys() {
    // DWG files can store an object whose own handle differs from its
    // object-map key; the drawing keeps it under the key.
    let mut d = sample();
    let mut a = d.objects[&Handle(0x103)].clone();
    a.handle = Handle(0x501);
    d.objects.insert(Handle(0x500), a);
    let mut b = d.objects[&Handle(0x104)].clone();
    b.handle = Handle(0x100); // collides with a real key
    d.objects.insert(Handle(0x502), b);
    d.rebuild_index();
    let bytes = to_bytes(&d, &WriteOptions::default());
    assert_same(&d, &read_bytes(&bytes).unwrap());
    assert_eq!(
        read_object(&bytes, Handle(0x502)).unwrap().unwrap().handle,
        Handle(0x100)
    );
}

#[test]
fn torn_magic_still_opens_from_trailer() {
    let d = sample();
    let mut b = to_bytes(&d, &WriteOptions::default());
    b[..8].copy_from_slice(b"garbage!");
    assert!(looks_native(&b));
    let r = crate::open_bytes(&b).unwrap();
    assert_same(&d, &r);
}

#[test]
fn lost_change_set_payload_falls_back_to_previous_state() {
    let path = temp("lostpayload");
    let d = sample();
    write_file(&d, &path, &WriteOptions::default()).unwrap();
    let before = std::fs::read(&path).unwrap();
    let set = ChangeSet {
        author: "a".into(),
        time: 1,
        changes: vec![Change::Delete(Handle(0x100))],
    };
    append_changes(&path, &set).unwrap();
    let after = std::fs::read(&path).unwrap();
    let hl = container::HEADER_LEN;
    // Directory and trailer reached the disk, the change set did not.
    let mut crashed = after.clone();
    crashed[..hl].copy_from_slice(&before[..hl]);
    for x in &mut crashed[before.len()..before.len() + 8] {
        *x = 0;
    }
    let r = read_bytes(&crashed).unwrap();
    assert!(r.get(Handle(0x100)).is_some(), "old state expected");
    assert!(r
        .diagnostics
        .iter()
        .any(|x| x.message.contains("incomplete")));

    // Same damage with both header copies already updated (bit rot): the
    // file still opens, without the damaged change set, and further
    // incremental saves are refused until a full save.
    let mut rotted = after.clone();
    for x in &mut rotted[before.len()..before.len() + 8] {
        *x = 0;
    }
    let r = read_bytes(&rotted).unwrap();
    assert!(r.get(Handle(0x100)).is_some());
    assert!(r.diagnostics.iter().any(|x| x.severity == Severity::Error));
    std::fs::write(&path, &rotted).unwrap();
    assert!(append_changes(&path, &set).is_err());
    write_file(&r, &path, &WriteOptions::default()).unwrap();
    assert!(append_changes(&path, &set).is_ok());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn later_duplicate_section_wins() {
    let d = sample();
    let page = |h: u64| {
        let mut out = Vec::new();
        let o = &d.objects[&Handle(h)];
        let mut w = TagWriter::new();
        rec::write_object(&mut w, o.handle, o);
        put_record(&mut out, w);
        out
    };
    let (p1, p2) = (page(0x100), page(0x101));
    let mut header = TagWriter::new();
    rec::write_header(&mut header, &d.header);
    let mut b = Builder::new(Encoding::current(true, Level::Default), false);
    b.section(SEC_META, meta_section(&d, 1, 1));
    b.section(SEC_HEADER, header.buf.clone());
    b.section("Objects.0", p1.clone());
    b.section("Objects.0", p2.clone());
    let r = read_bytes(&b.finish(1, 0)).unwrap();
    assert_eq!(
        r.objects.keys().copied().collect::<Vec<_>>(),
        vec![Handle(0x101)]
    );
}

#[test]
fn absurd_lengths_are_rejected_without_allocating() {
    let e = container::DirEntry {
        name: "Objects.0".into(),
        codec: Codec::Lz77,
        offset: 0,
        stored_len: 10,
        raw_len: 1 << 33,
        crc: 0,
        flags: 0,
    };
    assert!(container::read_entry(&[0x11; 10], &e).is_err());
    let e = container::DirEntry {
        codec: Codec::Raw,
        raw_len: 11,
        ..e
    };
    assert!(container::read_entry(&[0; 10], &e).is_err());
    assert!(container::decode_directory(&[0xFF, 0xFF, 0x03]).is_err());
}

#[test]
fn exhausted_generation_is_an_error_not_a_panic() {
    let path = temp("genmax");
    let d = sample();
    let mut b = Builder::new(Encoding::current(true, Level::Default), false);
    let mut header = TagWriter::new();
    rec::write_header(&mut header, &d.header);
    b.section(SEC_META, meta_section(&d, 0, 1));
    b.section(SEC_HEADER, header.buf.clone());
    b.section("Objects.0", Vec::new());
    std::fs::write(&path, b.finish(u64::MAX, 0)).unwrap();
    let set = ChangeSet::default();
    assert!(append_changes(&path, &set).is_err());
    assert!(write_file(&d, &path, &WriteOptions::default()).is_ok());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn audit_passes_on_every_kind_of_file_we_write() {
    // Snapshot, compressed and not.
    let d = sample();
    for compress in [true, false] {
        let b = to_bytes(
            &d,
            &WriteOptions {
                compress,
                ..Default::default()
            },
        );
        let rep = audit::audit(&b);
        assert!(rep.passed(), "{:?}", rep.failed().collect::<Vec<_>>());
        assert_eq!(rep.objects, d.objects.len());
        assert_eq!(rep.superseded_bytes(), 0);
        // The byte map covers the whole file exactly once.
        let covered: u64 = rep.regions.iter().map(|g| g.len).sum();
        assert_eq!(covered, b.len() as u64);
        assert!(rep.tags.binary_dxf_bytes > 0 && rep.tags.cad_bytes > 0);
    }
    // Journaled, then compacted.
    let path = temp("audit");
    write_file(&d, &path, &WriteOptions::default()).unwrap();
    for i in 0..3u64 {
        let set = ChangeSet {
            author: "a".into(),
            time: i,
            changes: vec![Change::Delete(Handle(0x103 + i))],
        };
        append_changes(&path, &set).unwrap();
    }
    let b = std::fs::read(&path).unwrap();
    let rep = audit::audit(&b);
    assert!(rep.passed(), "{:?}", rep.failed().collect::<Vec<_>>());
    assert_eq!(rep.change_sets, 3);
    assert!(
        rep.superseded_bytes() > 0,
        "old directories remain until compaction"
    );
    let back = read_bytes(&b).unwrap();
    write_file(&back, &path, &WriteOptions::default()).unwrap();
    let rep = audit::audit(&std::fs::read(&path).unwrap());
    assert!(rep.passed());
    assert_eq!((rep.change_sets, rep.superseded_bytes()), (0, 0));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn audit_reports_damage() {
    let d = sample();
    let good = to_bytes(
        &d,
        &WriteOptions {
            compress: false,
            preview: false,
            ..Default::default()
        },
    );
    let c = Container::parse(&good).unwrap();
    let page = c.entry("Objects.0").unwrap().clone();
    let map = c.entry("ObjectMap").unwrap().clone();

    let mut b = good.clone();
    b[(page.offset + page.stored_len / 2) as usize] ^= 0x01;
    let rep = audit::audit(&b);
    assert!(rep.failed().any(|x| x.name == "section data"));

    // Point the second map entry somewhere else (its offset varint).
    let mut b = good.clone();
    let (mut raw, _) = c.read(&map).unwrap();
    let mut p = 0;
    get_varint(&raw, &mut p).unwrap();
    for _ in 0..3 {
        get_varint(&raw, &mut p).unwrap();
    }
    get_varint(&raw, &mut p).unwrap(); // delta
    get_varint(&raw, &mut p).unwrap(); // page
    raw[p] ^= 0x02; // offset
    let at = map.offset as usize;
    b[at..at + raw.len()].copy_from_slice(&raw);
    let rep = audit::audit(&b);
    assert!(
        rep.failed().any(|x| x.name == "object map"),
        "{:?}",
        rep.checks
    );

    let mut b = good.clone();
    b[20] ^= 0xFF;
    let rep = audit::audit(&b);
    assert!(rep.failed().any(|x| x.name == "file header"));
    assert_eq!(rep.used, Some("trailer"));
}

#[test]
fn unknown_sections_and_kinds_are_tolerated() {
    let d = sample();
    let mut page = Vec::new();
    let o = &d.objects[&Handle(0x100)];
    let mut w = TagWriter::new();
    rec::write_object(&mut w, o.handle, o);
    put_record(&mut page, w);
    // An object of a kind a later revision might add.
    let mut w = TagWriter::new();
    w.str(0, "FUTURE_THING");
    w.handle(5, Handle(0x999));
    w.u16(rec::code::TYPE_CODE, 600);
    w.i16(rec::code::KIND, 99);
    w.p3(10, Vec3::X);
    let future_record = w.buf.clone();
    put_record(&mut page, w);
    let mut header = TagWriter::new();
    rec::write_header(&mut header, &d.header);
    let mut map = Vec::new();
    put_varint(&mut map, 0);
    let mut b = Builder::new(Encoding::current(true, Level::Default), false);
    b.section(SEC_META, meta_section(&d, 2, 1));
    b.section(SEC_HEADER, header.buf.clone());
    b.section(SEC_CLASSES, Vec::new());
    b.section("Objects.0", page.clone());
    b.section("Constraints", b"a section from the future".to_vec());
    b.section(SEC_OBJECT_MAP, map.clone());
    let bytes = b.finish(1, 0);
    let r = read_bytes(&bytes).unwrap();
    assert_eq!(r.objects.len(), 2);
    let f = &r.objects[&Handle(0x999)];
    assert_eq!(f.kind, ObjectKind::NewerKind(99));
    assert_eq!(f.type_name, "FUTURE_THING");
    let mut fields = TagWriter::new();
    fields.p3(10, Vec3::X);
    assert_eq!(f.unknown_tags, fields.buf);
    // Saving writes the object's record back exactly.
    let mut w = TagWriter::new();
    rec::write_object(&mut w, Handle(0x999), f);
    assert_eq!(w.buf, future_record);
    let rep = audit::audit(&bytes);
    assert!(rep
        .checks
        .iter()
        .any(|x| x.name == "unknown sections" && x.ok));
    assert!(rep.checks.iter().any(|x| x.name == "object records"
        && x.ok
        && x.detail.contains("1 objects of new kinds")));
    let again = to_bytes(&r, &WriteOptions::default());
    assert_eq!(read_bytes(&again).unwrap().objects[&Handle(0x999)], *f);
    let rep = audit::audit(&again);
    assert!(rep.passed(), "{:?}", rep.failed().collect::<Vec<_>>());
}

// ---------------------------------------------------------------------------
// Format 2.0
// ---------------------------------------------------------------------------

/// A value of the given type, for a required field.
fn any_value(w: &mut TagWriter, code: u16, t: u8) {
    use tags::ty;
    match t {
        ty::F64 => w.f64(code, 0.75),
        ty::I16 => w.i16(code, 0),
        ty::I32 => w.i32(code, 0),
        ty::I64 => w.i64(code, 0),
        ty::BOOL => w.bool(code, false),
        ty::STR => w.str(code, "x"),
        ty::HANDLE => w.handle(code, Handle(0x20)),
        ty::BYTES => w.bytes(code, &[1]),
        ty::P2 => w.p2(code, v2(1.5, -2.0)),
        ty::P3 => w.p3(code, v3(1.5, -2.0, 0.25)),
        other => panic!("group {code}: value type {other}"),
    }
}

/// The schema's default, written out in the field's type.
fn default_value(w: &mut TagWriter, code: u16, d: schema::Def) {
    use schema::Def;
    match d {
        Def::F64(v) => w.f64(code, v),
        Def::I16(v) => w.i16(code, v),
        Def::I32(v) => w.i32(code, v),
        Def::Bool(v) => w.bool(code, v),
        Def::Str(v) => w.str(code, v),
        Def::P2(v) => w.p2(code, v2(v[0], v[1])),
        Def::P3(v) => w.p3(code, v3(v[0], v[1], v[2])),
        Def::Req | Def::Null | Def::Cond => {}
    }
}

/// The fields of one part of a record as the schema describes them:
/// required fields with some value, and (when `defaults`) every field that
/// has a default written out with it. Lists stay empty, and fields that
/// are absent when null or depend on others are left out.
fn schema_part(w: &mut TagWriter, spec: &schema::KindSpec, defaults: bool) {
    use schema::{Def, Role};
    for f in spec.fields {
        if f.role == Role::Item {
            continue;
        }
        match (f.def, f.role) {
            (Def::Null | Def::Cond, _) => {}
            (Def::Req, Role::Count) => w.i32(f.code, 0),
            (Def::Req, _) => any_value(w, f.code, f.ty),
            (d, _) if defaults => {
                assert_eq!(
                    std::mem::discriminant(&d),
                    std::mem::discriminant(&match f.ty {
                        tags::ty::F64 => Def::F64(0.0),
                        tags::ty::I16 => Def::I16(0),
                        tags::ty::I32 => Def::I32(0),
                        tags::ty::BOOL => Def::Bool(false),
                        tags::ty::STR => Def::Str(""),
                        tags::ty::P2 => Def::P2([0.0; 2]),
                        tags::ty::P3 => Def::P3([0.0; 3]),
                        _ => Def::Req,
                    }),
                    "{} group {}: the default is not of the field's type",
                    spec.name,
                    f.code
                );
                default_value(w, f.code, d)
            }
            _ => {}
        }
    }
}

#[test]
fn the_schema_matches_the_record_code() {
    // Every kind this version writes is described, and nothing else.
    for k in rec::kind::UNSUPPORTED..=rec::kind::LAST {
        assert!(schema::kind(k).is_some(), "kind {k} has no schema");
    }
    assert!(schema::KINDS
        .iter()
        .all(|s| s.kind < 0 || s.kind <= rec::kind::LAST));
    let entity = schema::kind(-2).unwrap();
    for k in rec::kind::UNSUPPORTED..=rec::kind::LAST {
        let spec = schema::kind(k).unwrap();
        let record = |defaults: bool| {
            let mut w = TagWriter::new();
            // The object part: key and a type code whose name is implied.
            w.handle(5, Handle(0x20));
            w.u16(rec::code::TYPE_CODE, 0x13);
            schema_part(&mut w, entity, defaults);
            w.i16(rec::code::KIND, k);
            schema_part(&mut w, spec, defaults);
            w.buf
        };
        let (bare, full) = (record(false), record(true));
        let (key, a, problem) = rec::read_object_checked(&bare).unwrap();
        assert!(problem.is_none(), "{}: {problem:?}", spec.name);
        let (_, b, problem) = rec::read_object_checked(&full).unwrap();
        assert!(problem.is_none(), "{}: {problem:?}", spec.name);
        assert!(
            a.unknown_tags.is_empty() && b.unknown_tags.is_empty(),
            "{}: fields left unread",
            spec.name
        );
        assert!(!matches!(
            a.kind,
            ObjectKind::Damaged(_) | ObjectKind::NewerKind(_)
        ));
        // Writing a default out changes nothing...
        assert_eq!(a, b, "{}", spec.name);
        // ...and the writer leaves every default out, in schema order.
        let mut w = TagWriter::new();
        rec::write_object(&mut w, key, &a);
        assert_eq!(w.buf, bare, "{}", spec.name);
    }
}

#[test]
fn every_file_describes_its_records() {
    let bytes = to_bytes(&sample(), &WriteOptions::default());
    let f = NativeFile::from_bytes(&bytes).unwrap();
    let kinds = f.schema().unwrap();
    let ours: Vec<_> = schema::KINDS.iter().map(schema::as_file_kind).collect();
    assert_eq!(kinds, ours);
    assert_eq!(
        schema::field_name(&kinds, Some(rec::kind::LINE), 11),
        Some("end")
    );
    assert_eq!(
        schema::field_name(&kinds, Some(rec::kind::LINE), 370),
        Some("lineweight code")
    );
    // Defaults are stored in the field's own type.
    let ellipse = kinds.iter().find(|k| k.name == "ELLIPSE").unwrap();
    let end = ellipse.fields.iter().find(|x| x.code == 42).unwrap();
    assert_eq!(end.default, Some(tags::Value::F64(std::f64::consts::TAU)));
    assert_eq!(f.info().schema_kinds, schema::KINDS.len());
}

#[test]
fn files_written_by_format_1_1_still_read() {
    // Written by the format 1.1 sources (commit e75087e) from `sample()` as
    // it was then (no drawing properties), followed by one change set: line
    // 0x100 moved, circle 0x101 deleted, $INSUNITS set to 4. If `sample()`
    // changes, compare against what it held then.
    let bytes: &[u8] = include_bytes!("../../tests/fixtures/sample-1.1.cad");
    let f = NativeFile::from_bytes(bytes).unwrap();
    assert_eq!((f.format(), f.generation()), ((1, 1), 2));
    let d = f.drawing().unwrap();
    assert!(
        d.diagnostics.iter().all(|x| x.severity == Severity::Info),
        "{:?}",
        d.diagnostics
    );
    let mut want = sample();
    // 1.1 had no drawing properties or part data, and kept no class
    // counts or versions (2.1).
    want.header.properties = Properties::default();
    for c in &mut want.classes {
        (c.instance_count, c.dwg_version, c.maintenance_version) = (0, 0, 0);
    }
    if let ObjectKind::BlockHeader(b) = &mut want.objects.get_mut(&Handle(0x40)).unwrap().kind {
        b.part = Part::default();
    }
    let mut line = want.objects[&Handle(0x100)].clone();
    if let ObjectKind::Line(l) = &mut line.kind {
        l.end = v3(120.0, 60.0, 0.0);
    }
    let mut header = want.header.clone();
    header.insunits = Some(4);
    apply(
        &mut want,
        &ChangeSet {
            author: "fixture".into(),
            time: 1_700_000_000,
            changes: vec![
                Change::Put(Box::new(line.clone())),
                Change::Delete(Handle(0x101)),
                Change::Header(Box::new(header)),
            ],
        },
    );
    assert_same(&want, &d);
    assert_eq!(f.object(Handle(0x100)).unwrap(), Some(line));
    assert_eq!(f.object(Handle(0x101)).unwrap(), None);
    assert_eq!(f.header().unwrap().insunits, Some(4));
    let rep = audit::audit(bytes);
    assert!(rep.passed(), "{:?}", rep.failed().collect::<Vec<_>>());
    assert!(!f.verify_chain().present);
    assert_eq!(f.info().schema_kinds, 0);

    // A change set appended to it is written the 1.x way, so 1.x readers
    // can still read the file: DWG LZ77 or stored, no section flags, one
    // directory.
    let path = temp("fixture11");
    std::fs::write(&path, bytes).unwrap();
    let mut moved = want.objects[&Handle(0x102)].clone();
    moved.owner = Handle(0x1F);
    append_changes(
        &path,
        &ChangeSet {
            author: "2.0".into(),
            time: 1,
            changes: vec![Change::Put(Box::new(moved.clone()))],
        },
    )
    .unwrap();
    let grown = std::fs::read(&path).unwrap();
    let c = Container::parse(&grown).unwrap();
    assert_eq!(
        (c.header.major, c.header.minor, c.header.generation),
        (1, 1, 3)
    );
    let e = c.entry("Journal.3").unwrap();
    assert!(matches!(e.codec, Codec::Lz77 | Codec::Raw) && e.flags == 0);
    let n = grown.len();
    let back = FileHeader::decode(&grown[n - container::HEADER_LEN..]).unwrap();
    assert_eq!(back, c.header, "one directory, both headers alike");
    assert!(container::decode_directory(
        &grown[c.header.dir_offset as usize..][..c.header.dir_len as usize]
    )
    .is_ok());
    assert_eq!(read_file(&path).unwrap().objects[&Handle(0x102)], moved);
    let rep = audit::audit(&grown);
    assert!(rep.passed(), "{:?}", rep.failed().collect::<Vec<_>>());
    // A full save writes the current version.
    let d = read_file(&path).unwrap();
    write_file(&d, &path, &WriteOptions::default()).unwrap();
    let f = NativeFile::open(&path).unwrap();
    assert_eq!(f.format(), (FORMAT_MAJOR, FORMAT_MINOR));
    assert_same(&d, &f.drawing().unwrap());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn record_sections_are_split_and_deflated() {
    let bytes = to_bytes(&sample(), &WriteOptions::default());
    let info = inspect(&bytes).unwrap();
    assert_eq!(info.format, (FORMAT_MAJOR, FORMAT_MINOR));
    let page = info
        .sections
        .iter()
        .find(|s| s.name == "Objects.0")
        .unwrap();
    assert_eq!(page.codec, "DEFLATE");
    assert!(page.split && page.crc_ok && page.stored < page.raw);
    for name in ["Meta", "Header", "Schema", "Digest", "Recovery"] {
        assert!(
            info.sections.iter().any(|s| s.name == name),
            "{name} missing"
        );
    }
    // Uncompressed files store everything as it is, without flags.
    let raw = to_bytes(
        &sample(),
        &WriteOptions {
            compress: false,
            ..Default::default()
        },
    );
    let c = Container::parse(&raw).unwrap();
    assert!(c
        .entries
        .iter()
        .all(|e| e.codec == Codec::Raw && e.flags == 0));
    assert_same(&read_bytes(&raw).unwrap(), &read_bytes(&bytes).unwrap());
    // Every level reads back the same.
    for level in [Level::Fast, Level::Best] {
        let b = to_bytes(
            &sample(),
            &WriteOptions {
                level,
                ..Default::default()
            },
        );
        assert_same(&sample(), &read_bytes(&b).unwrap());
    }
}

/// Rewrite the directory of a file after changing its entries, with new
/// directories and headers of the given version.
fn retail(bytes: &[u8], version: (u16, u16), f: impl Fn(&mut DirEntry)) -> Vec<u8> {
    let c = Container::parse(bytes).unwrap();
    let mut entries = c.entries.clone();
    entries.iter_mut().for_each(f);
    let end = entries
        .iter()
        .map(|e| e.offset + e.stored_len)
        .max()
        .unwrap() as usize;
    let mut out = bytes[..end].to_vec();
    let tail = container::directory_and_trailer(
        end as u64,
        &entries,
        FileHeader {
            major: version.0,
            minor: version.1,
            flags: c.header.flags,
            generation: c.header.generation,
            ..Default::default()
        },
    );
    out.extend_from_slice(&tail.bytes);
    out[..container::HEADER_LEN].copy_from_slice(&tail.front);
    out
}

/// Rewrite one directory entry in place (both copies of the directory)
/// and fix the headers to match.
fn patch_entry(bytes: &[u8], name: &str, f: impl Fn(&mut DirEntry)) -> Vec<u8> {
    let c = Container::parse(bytes).unwrap();
    retail(bytes, (c.header.major, c.header.minor), |e| {
        if e.name == name {
            f(e)
        }
    })
}

/// The same sections laid out as a file of another version (the
/// directory and headers as that version writes them).
fn as_version(bytes: &[u8], major: u16, minor: u16) -> Vec<u8> {
    retail(bytes, (major, minor), |_| {})
}

#[test]
fn sections_a_reader_cannot_decode_are_reported_and_skipped() {
    let opts = WriteOptions {
        recovery: false,
        ..Default::default()
    };
    let bytes = to_bytes(&sample(), &opts);
    for (what, patched) in [
        (
            "flags",
            patch_entry(&bytes, "DwgPreserved", |e| e.flags |= 1 << 7),
        ),
        (
            "encrypted",
            patch_entry(&bytes, "DwgPreserved", |e| {
                e.flags |= container::section_flag::ENCRYPTED
            }),
        ),
        (
            "codec",
            patch_entry(&bytes, "DwgPreserved", |e| e.codec = Codec::Other(9)),
        ),
    ] {
        let d = read_bytes(&patched).unwrap();
        assert!(
            d.diagnostics
                .iter()
                .any(|x| x.severity == Severity::Error && x.message.contains("DwgPreserved")),
            "{what}: {:?}",
            d.diagnostics
        );
        assert_eq!(d.objects.len(), sample().objects.len(), "{what}");
        let rep = audit::audit(&patched);
        assert!(rep.failed().any(|c| c.name == "section encoding"), "{what}");
    }
    // Informational flags (bits 32 and up) are ignored.
    let patched = patch_entry(&bytes, "DwgPreserved", |e| e.flags |= 1 << 40);
    assert_same(&sample(), &read_bytes(&patched).unwrap());
}

#[test]
fn either_copy_of_the_directory_is_enough() {
    let bytes = to_bytes(&sample(), &WriteOptions::default());
    let front = FileHeader::decode(&bytes).unwrap();
    let n = bytes.len();
    let back = FileHeader::decode(&bytes[n - container::HEADER_LEN..]).unwrap();
    assert!(front.same_state(&back) && front.dir_offset < back.dir_offset);
    // The recovery data lies between the two copies.
    let c = Container::parse(&bytes).unwrap();
    let rec_e = c.entry("Recovery").unwrap();
    assert_eq!(front.dir_offset + front.dir_len as u64, rec_e.offset);
    assert_eq!(rec_e.offset + rec_e.stored_len, back.dir_offset);
    for (target, used) in [(front, true), (back, false)] {
        let mut b = bytes.clone();
        b[target.dir_offset as usize + 3] ^= 0x20;
        let d = read_bytes(&b).unwrap();
        assert_same(&sample(), &d);
        // The reader notices only when it needed the damaged copy; the
        // audit always does.
        assert_eq!(
            d.diagnostics
                .iter()
                .any(|x| x.message.contains("one copy of the directory is damaged")),
            used
        );
        let rep = audit::audit(&b);
        assert!(
            rep.failed()
                .any(|x| x.name == "directory copies" || x.name == "directory"),
            "{:?}",
            rep.checks
        );
    }
    let rep = audit::audit(&bytes);
    assert!(rep.passed(), "{:?}", rep.failed().collect::<Vec<_>>());
    assert!(rep
        .regions
        .iter()
        .any(|g| g.label == "directory, second copy"));
    assert_eq!(rep.superseded_bytes(), 0);
    // A copy cut short anywhere after the first directory copy still opens
    // in full, through the front header.
    for cut in [
        rec_e.offset as usize + 5,
        back.dir_offset as usize + 2,
        n - 10,
    ] {
        let d = read_bytes(&bytes[..cut]).unwrap();
        assert_same(&sample(), &d);
    }
    // Without recovery data the copies are side by side.
    let plain = to_bytes(
        &sample(),
        &WriteOptions {
            recovery: false,
            ..Default::default()
        },
    );
    let f = FileHeader::decode(&plain).unwrap();
    let t = FileHeader::decode(&plain[plain.len() - container::HEADER_LEN..]).unwrap();
    assert_eq!(f.dir_offset + f.dir_len as u64, t.dir_offset);
    let rep = audit::audit(&plain);
    assert!(rep.passed(), "{:?}", rep.failed().collect::<Vec<_>>());
}

#[test]
fn recovery_data_rebuilds_damaged_bytes() {
    let d = sample();
    let bytes = to_bytes(&d, &WriteOptions::default());
    let c = Container::parse(&bytes).unwrap();
    let page = c.entry("Objects.0").unwrap().clone();
    let header = c.entry("Header").unwrap().clone();
    let rec_e = c.entry("Recovery").unwrap().clone();
    let rec_data = c.read(&rec_e).unwrap().0;
    let r = recovery::decode(&rec_data).unwrap();
    assert_eq!(r.start, container::HEADER_LEN);
    // It covers the sections; the first directory copy follows them.
    assert_eq!((r.start + r.len) as u64, c.header.dir_offset);
    assert_eq!(c.header.dir_offset + c.header.dir_len as u64, rec_e.offset);

    // A run of damage starting inside the header.
    let mut b = bytes.clone();
    let at = header.offset as usize + 2;
    let run = r.longest_run().min(page.offset as usize + 40 - at);
    for x in &mut b[at..at + run] {
        *x = !*x;
    }
    let back = read_bytes(&b).unwrap();
    assert_same(&d, &back);
    assert!(back
        .diagnostics
        .iter()
        .any(|x| x.message.contains("rebuilt from the recovery data")));
    // One object, through the open file, is rebuilt the same way.
    let f = NativeFile::from_bytes(&b).unwrap();
    assert_eq!(
        f.object(Handle(0x100)).unwrap().as_ref(),
        d.objects.get(&Handle(0x100))
    );
    assert_eq!(f.properties().unwrap(), d.header.properties);
    let info = f.info().recovery.unwrap();
    assert!(info.damaged > 0 && info.unrepairable == 0);
    let rep = audit::audit(&b);
    assert!(rep.failed().any(|x| x.name == "recovery data"));

    // Damage from the end of the last section into the first directory
    // copy: the second copy is used, and the section is rebuilt.
    let last = c
        .entries
        .iter()
        .filter(|e| e.name != "Recovery")
        .max_by_key(|e| e.offset)
        .unwrap()
        .clone();
    let mut b = bytes.clone();
    let from = (last.offset + last.stored_len) as usize - 20;
    for x in &mut b[from..from + 40] {
        *x ^= 0xA5;
    }
    let back = read_bytes(&b).unwrap();
    assert_same(&d, &back);
    assert!(back
        .diagnostics
        .iter()
        .any(|x| x.message.contains("one copy of the directory is damaged")));
    // Damage that reaches the recovery data's parity too: blocks whose
    // parity is intact are still rebuilt (the section checks its own
    // layout; each rebuilt block is checked against its CRC).
    let mut b = bytes.clone();
    let parity_at = rec_e.offset as usize + rec_data.len() - r.stripes * r.block;
    b[parity_at + r.block * (r.stripes - 1)] ^= 0xFF; // the last stripe's parity
    b[header.offset as usize + 1] ^= 0xFF; // a block of the first stripes
    let f = NativeFile::from_bytes(&b).unwrap();
    assert_eq!(f.properties().unwrap(), d.header.properties);

    // Too much damage in one stripe: reported as before, nothing invented.
    let mut b = bytes.clone();
    let mut i = page.offset as usize;
    while i < (page.offset + page.stored_len) as usize {
        b[i] ^= 0xFF;
        i += r.block * r.stripes / 2;
    }
    let back = read_bytes(&b).unwrap();
    assert!(back
        .diagnostics
        .iter()
        .any(|x| x.severity != Severity::Info && x.message.contains("page")));
}

#[test]
fn the_hash_chain_covers_the_snapshot_and_every_change_set() {
    let path = temp("chain");
    let d = sample();
    write_file(&d, &path, &WriteOptions::default()).unwrap();
    let f = NativeFile::open(&path).unwrap();
    let ch = f.verify_chain();
    assert!(ch.ok() && ch.links.is_empty(), "{:?}", ch.problems);
    let head0 = ch.head.clone();
    assert_eq!(f.info().chain_head.as_deref(), Some(head0.as_str()));
    for i in 0..2u64 {
        let mut o = d.objects[&Handle(0x100)].clone();
        o.owner = Handle(0x1F + i);
        append_changes(
            &path,
            &ChangeSet {
                author: "a".into(),
                time: 5 + i,
                changes: vec![Change::Put(Box::new(o))],
            },
        )
        .unwrap();
    }
    let bytes = std::fs::read(&path).unwrap();
    let f = NativeFile::from_bytes(&bytes).unwrap();
    let ch = f.verify_chain();
    assert!(ch.ok(), "{:?}", ch.problems);
    assert_eq!(ch.links.iter().map(|l| l.0).collect::<Vec<_>>(), vec![2, 3]);
    assert_ne!(ch.head, head0);
    assert_eq!(f.info().chain_head, Some(ch.head.clone()));
    let rep = audit::audit(&bytes);
    assert!(rep.passed(), "{:?}", rep.failed().collect::<Vec<_>>());

    // A section whose records were changed and whose CRC was fixed up
    // passes every CRC but not the chain.
    let mut hv = d.header.clone();
    hv.properties.author = "someone else".into();
    let mut w = TagWriter::new();
    rec::write_header(&mut w, &hv);
    let x = container::encode_section(&w.buf, false, Encoding::current(true, Level::Default));
    let mut forged = bytes.clone();
    let at = forged.len();
    forged.extend_from_slice(&x.stored);
    let forged = patch_entry(&forged, "Header", |h| {
        h.offset = at as u64;
        h.stored_len = x.stored.len() as u64;
        h.raw_len = w.buf.len() as u64;
        h.crc = x.crc;
        h.codec = x.codec;
        h.flags = x.flags;
    });
    // `patch_entry` keeps bytes up to the last section, so the new header
    // data survives; every CRC checks out.
    let f = NativeFile::from_bytes(&forged).unwrap();
    assert_eq!(f.properties().unwrap().author, "someone else");
    let ch = f.verify_chain();
    assert!(!ch.ok() && !ch.snapshot_ok, "{ch:?}");
    let rep = audit::audit(&forged);
    assert!(rep.failed().any(|x| x.name == "hash chain"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn drawing_properties_round_trip_and_travel_in_change_sets() {
    let path = temp("props");
    let d = sample();
    write_file(&d, &path, &WriteOptions::default()).unwrap();
    let f = NativeFile::open(&path).unwrap();
    assert_eq!(f.properties().unwrap(), d.header.properties);
    let mut h = d.header.clone();
    h.properties.set("revision", "D");
    h.properties.set("Material", "");
    h.properties.set("Finish", "galvanised");
    append_changes(
        &path,
        &ChangeSet {
            author: "b".into(),
            time: 9,
            changes: vec![Change::Header(Box::new(h.clone()))],
        },
    )
    .unwrap();
    let p = NativeFile::open(&path).unwrap().properties().unwrap();
    assert_eq!(p.revision, "D");
    assert_eq!(
        p.custom,
        vec![
            ("Client".to_string(), "Ünïcödé Ltd".to_string()),
            ("Finish".to_string(), "galvanised".to_string())
        ]
    );
    assert_eq!(read_file(&path).unwrap().header, h);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn the_format_document_lists_the_schema() {
    const DOC: &str = include_str!("../../../../docs/FORMAT.md");
    let table = schema::markdown_table();
    assert!(
        DOC.contains(&table),
        "docs/FORMAT.md §8.2 differs from the schema; replace it with the output of `cad schema`:\n{table}"
    );
    assert!(DOC.starts_with(&format!(
        "# Native file format (`.cad`) — version {FORMAT_MAJOR}.{FORMAT_MINOR}"
    )));
}

#[test]
fn records_for_1x_files_are_written_exactly_as_1_1_wrote_them() {
    // Every object record in the 1.1 fixture, decoded and written again in
    // the 1.x form, must come out byte for byte as the 1.1 code wrote it:
    // change sets appended to 1.x files stay readable by 1.x readers.
    let bytes: &[u8] = include_bytes!("../../tests/fixtures/sample-1.1.cad");
    let c = Container::parse(bytes).unwrap();
    let page = c.read(c.entry("Objects.0").unwrap()).unwrap().0;
    let mut kinds = std::collections::HashSet::new();
    let mut n = 0;
    for r in records(&page).unwrap() {
        let (key, o) = rec::read_object(r).unwrap();
        kinds.insert(std::mem::discriminant(&o.kind));
        let mut w = TagWriter::for_major(1);
        rec::write_object(&mut w, key, &o);
        assert_eq!(w.buf, r, "object {key} ({})", o.type_name);
        // And the 2.0 form is never larger.
        let mut w2 = TagWriter::for_major(2);
        rec::write_object(&mut w2, key, &o);
        assert!(w2.buf.len() <= r.len());
        n += 1;
    }
    assert!(
        n > 30 && kinds.len() > 25,
        "{n} records, {} kinds",
        kinds.len()
    );
    // The change set the fixture carries, too.
    let j = c.read(c.entry("Journal.2").unwrap()).unwrap().0;
    let set = decode_journal(&j).unwrap();
    let again = encode_journal(&set, 2, 1, None);
    assert_eq!(again, j);
}

#[test]
#[ignore]
fn print_schema_table() {
    print!("{}", schema::markdown_table());
}
