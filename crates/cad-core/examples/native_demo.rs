//! Build a drawing in code on the default layers (the sample plate), save
//! it as native `.blc`, lock it, make an incremental save, and read it
//! back.
//!
//!     cargo run -p cad-core --example native_demo -- /tmp/plate.blc

use cad_core::geom::Vec2;
use cad_core::model::*;
use cad_core::native::{self, lock, Change, ChangeSet};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "plate.blc".into());
    let mut d = cad_core::samples::plate();

    // Lock, save, then save one change incrementally.
    let held = match lock::acquire(&path)? {
        lock::Acquire::Acquired(l) => l,
        lock::Acquire::HeldBy(who) => {
            return Err(format!("{path} is open by {}@{}", who.user, who.host).into());
        }
    };
    let g = native::write_file(&d, &path, &Default::default())?;
    println!(
        "saved {path} (generation {g}), lock {}",
        held.path().display()
    );

    // The change: a revision note on NOTES. The new object and the header
    // (whose handle seed moved) go into the change set.
    let notes = d.layer_by_name("NOTES").map(|(h, _)| h).unwrap_or_default();
    let rev = d.add_entity(
        notes,
        ObjectKind::Text(Text::new(Vec2::new(150.0, -22.0), 3.5, "REV A")),
    )?;
    let g = native::append_changes(
        &path,
        &ChangeSet {
            author: held.info().user.clone(),
            time: 0,
            changes: vec![
                Change::Put(Box::new(d.objects[&rev].clone())),
                Change::Header(Box::new(d.header.clone())),
            ],
        },
    )?;
    println!("appended change set (generation {g})");

    let back = cad_core::open(&path)?;
    if let Some(ObjectKind::Text(t)) = back.get(rev).map(|o| &o.kind) {
        println!("after reload: {:?} on layer NOTES", t.value);
    }
    let layers: Vec<String> = back.layers().iter().map(|(_, l)| l.name.clone()).collect();
    println!("layers: {}", layers.join(", "));
    if let Some(x) = back.objects.values().find_map(|o| o.xdata_for("DEMO_APP")) {
        println!("outline extended data after reload: {:?}", x.items);
    }
    if std::env::args().any(|a| a == "--keep-lock") {
        std::mem::forget(held); // leave the .bdl behind for `cad presence`
    }
    Ok(())
}
