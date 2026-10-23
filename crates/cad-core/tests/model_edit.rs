//! The drawing database's editing and undo (`blackcad_model::model::edit`),
//! tried on drawings made from BlackCAD's templates. The tests live here
//! because the templates (`cad_core::standards`) are part of the core.

mod change_tests {
    use cad_core::geom::Vec3;
    use cad_core::model::*;
    use cad_core::{new_drawing, Template};

    fn line(x: f64) -> ObjectKind {
        ObjectKind::Line(Line {
            start: Vec3::new(x, 0.0, 0.0),
            end: Vec3::new(x, 10.0, 0.0),
            extrusion: Vec3::new(0.0, 0.0, 1.0),
            ..Default::default()
        })
    }

    fn index_is_current(d: &Drawing) {
        let mut fresh = d.clone();
        fresh.rebuild_index();
        assert_eq!(d.index, fresh.index);
    }

    /// Model space listing its entities, as R2004+ files do.
    fn listed() -> Drawing {
        let mut d = new_drawing(Template::Metric);
        let a = d.add_entity(Handle::NULL, line(0.0)).unwrap();
        let ms = d.model_space();
        if let Some(Object {
            kind: ObjectKind::BlockHeader(b),
            ..
        }) = d.objects.get_mut(&ms)
        {
            b.entities = vec![a];
        }
        d
    }

    #[test]
    fn edits_undo_and_redo_exactly() {
        for mut d in [new_drawing(Template::Metric), listed()] {
            let mut states = vec![d.clone()];
            let mut changes = Vec::new();
            let mut hs = Vec::new();
            for i in 0..12 {
                let s = d.begin_change();
                match i % 4 {
                    0 | 1 => hs.push(d.add_entity(Handle::NULL, line(i as f64)).unwrap()),
                    2 => {
                        let h = hs.remove(0);
                        d.erase_entity(h).unwrap();
                    }
                    _ => {
                        let l = match d.layer_by_name("L") {
                            Some((h, _)) => h,
                            None => d.add_layer("L", Color::Index(1), Handle::NULL, Lineweight::Default).unwrap(),
                        };
                        d.set_current_layer(l).unwrap();
                        let h = *hs.last().unwrap();
                        d.objects.get_mut(&h).unwrap().entity.as_mut().unwrap().layer = l;
                    }
                }
                let c = d.end_change(s);
                assert!(!c.is_empty());
                changes.push(c);
                index_is_current(&d);
                states.push(d.clone());
            }
            // Back to the start, one step at a time, then forward again.
            let mut redo = Vec::new();
            while let Some(c) = changes.pop() {
                redo.push(d.revert(c));
                states.pop();
                assert_eq!(&d, states.last().unwrap());
                index_is_current(&d);
            }
            let mut again = vec![d.clone()];
            while let Some(c) = redo.pop() {
                d.revert(c);
                index_is_current(&d);
                again.push(d.clone());
            }
            assert_eq!(again.len(), 13);
            assert_eq!(d.model_space_entities().len(), again[12].model_space_entities().len());
        }
    }

    #[test]
    fn merged_changes_undo_as_one() {
        let mut d = listed();
        let start = d.clone();
        let mut group = Change::default();
        let mut hs = Vec::new();
        for i in 0..5 {
            let s = d.begin_change();
            hs.push(d.add_entity(Handle::NULL, line(i as f64)).unwrap());
            group.merge(d.end_change(s));
        }
        let s = d.begin_change();
        d.erase_entity(hs[2]).unwrap();
        d.objects.get_mut(&hs[0]).unwrap().entity.as_mut().unwrap().linetype_scale = 2.0;
        group.merge(d.end_change(s));
        let end = d.clone();
        let again = d.revert(group);
        assert_eq!(d, start);
        index_is_current(&d);
        d.revert(again);
        assert_eq!(d, end);
        index_is_current(&d);
    }

    #[test]
    fn header_variables_come_back() {
        let mut d = new_drawing(Template::Metric);
        let start = d.clone();
        let mut group = Change::default();
        // Moves the handle seed only: the variables are not kept...
        let s = d.begin_change();
        d.add_entity(Handle::NULL, line(0.0)).unwrap();
        let c = d.end_change(s);
        assert!(c.changes_header() && !c.keeps_raw_header());
        group.merge(c);
        // ...until a later step changes them.
        let s = d.begin_change();
        d.header.raw.push(("USERI1".into(), "5".into()));
        group.merge(d.end_change(s));
        let end = d.clone();
        let again = d.revert(group);
        assert_eq!(d, start);
        d.revert(again);
        assert_eq!(d, end);
    }

    #[test]
    fn an_edit_costs_what_it_touches() {
        // A model space listing 5,000 entities: adding one records the
        // place in the list, not the list.
        let mut d = listed();
        for i in 0..5000 {
            d.add_entity(Handle::NULL, line(i as f64)).unwrap();
        }
        let s = d.begin_change();
        let h = d.add_entity(Handle::NULL, line(-1.0)).unwrap();
        let c = d.end_change(s);
        assert!(c.approx_bytes() < 4096, "{}", c.approx_bytes());
        let s = d.begin_change();
        d.erase_entity(h).unwrap();
        let c = d.end_change(s);
        assert!(c.approx_bytes() < 4096, "{}", c.approx_bytes());
    }
}

mod erase_tests {
    use cad_core::geom::Vec3;
    use cad_core::model::*;
    use cad_core::{new_drawing, Template};

    #[test]
    fn erased_entities_leave_the_drawing_and_its_lists() {
        let mut d = new_drawing(Template::Metric);
        let line = |x: f64| {
            ObjectKind::Line(Line {
                start: Vec3::new(x, 0.0, 0.0),
                end: Vec3::new(x, 10.0, 0.0),
                extrusion: Vec3::new(0.0, 0.0, 1.0),
                ..Default::default()
            })
        };
        let a = d.add_entity(Handle::NULL, line(0.0)).unwrap();
        let b = d.add_entity(Handle::NULL, line(5.0)).unwrap();
        assert_eq!(d.model_space_entities().len(), 2);
        d.erase_entity(a).unwrap();
        assert!(d.get(a).is_none());
        let left: Vec<Handle> = d.model_space_entities().iter().map(|o| o.handle).collect();
        assert_eq!(left, vec![b]);
        assert_eq!(cad_core::render::model_space(&d).items.len(), 1);
        assert!(d.erase_entity(a).is_err());
        // Not an entity.
        let layer = d.current_layer().unwrap();
        assert!(d.erase_entity(layer).is_err());
    }
}
