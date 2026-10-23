//! The drawing's objects by handle, able to record what an edit changes.
//!
//! Reading goes through `Deref` to the map. Every way of changing it is a
//! method here, so while a journal is open ([`ObjectMap::start_journal`])
//! the first time an edit touches an object, the object as it was is kept.
//! That record is the edit's undo: [`Drawing::revert`](super::Drawing::revert)
//! puts those objects back and returns the record that puts them forward
//! again. An edit costs memory in proportion to what it touches, not to
//! the size of the drawing.
//!
//! A block record's entity list (R2004+ files list every entity of model
//! space in it) is the exception to keeping whole objects: adding or
//! erasing one entity would copy the whole list. The list is changed
//! through [`ObjectMap::list_insert`] and [`ObjectMap::list_remove`]
//! instead, which record the one position changed.

use super::{Handle, Object, ObjectKind};
use std::collections::BTreeMap;
use std::ops::Deref;

/// One change to a block record's entity list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOp {
    Insert(usize, Handle),
    Remove(usize, Handle),
}

impl ListOp {
    fn inverse(self) -> ListOp {
        match self {
            ListOp::Insert(at, h) => ListOp::Remove(at, h),
            ListOp::Remove(at, h) => ListOp::Insert(at, h),
        }
    }
}

/// What an edit did to one object.
///
/// To take the edit back: put `prior` back (when there is one), then undo
/// `list_ops` from the last. List changes are recorded only until the
/// object is kept whole, so `prior` is the object with those changes made.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Touch {
    /// The object before the edit (`Some(None)`: it did not exist).
    pub prior: Option<Option<Object>>,
    pub list_ops: Vec<ListOp>,
}

/// The objects an edit touched, as they were.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Journal {
    pub touched: BTreeMap<Handle, Touch>,
}

impl Journal {
    pub fn is_empty(&self) -> bool {
        self.touched.is_empty()
    }

    /// Add the record of a later edit, so the two undo as one.
    pub fn merge(&mut self, later: Journal) {
        for (h, t) in later.touched {
            match self.touched.get_mut(&h) {
                None => {
                    self.touched.insert(h, t);
                }
                // A whole object kept by the earlier edit already undoes
                // everything after it.
                Some(e) if e.prior.is_some() => {}
                Some(e) => {
                    e.list_ops.extend(t.list_ops);
                    e.prior = t.prior;
                }
            }
        }
    }

    /// Bytes the record holds, roughly.
    pub fn approx_bytes(&self) -> usize {
        self.touched
            .values()
            .map(|t| {
                64 + t.list_ops.len() * std::mem::size_of::<ListOp>()
                    + match &t.prior {
                        Some(Some(o)) => o.approx_bytes(),
                        _ => 0,
                    }
            })
            .sum()
    }
}

/// The drawing's objects (see the module documentation).
#[derive(Default)]
pub struct ObjectMap {
    map: BTreeMap<Handle, Object>,
    journal: Option<Journal>,
}

impl ObjectMap {
    pub fn new() -> ObjectMap {
        ObjectMap::default()
    }

    /// Start recording what changes.
    pub fn start_journal(&mut self) {
        self.journal = Some(Journal::default());
    }

    /// Stop recording; what changed since [`ObjectMap::start_journal`].
    pub fn take_journal(&mut self) -> Journal {
        self.journal.take().unwrap_or_default()
    }

    pub fn journaling(&self) -> bool {
        self.journal.is_some()
    }

    /// Keep the object as it is now, if this is the first time it is
    /// changed since the journal started.
    fn keep(&mut self, h: Handle) {
        if let Some(j) = &mut self.journal {
            let t = j.touched.entry(h).or_default();
            if t.prior.is_none() {
                t.prior = Some(self.map.get(&h).cloned());
            }
        }
    }

    pub fn get_mut(&mut self, h: &Handle) -> Option<&mut Object> {
        if self.map.contains_key(h) {
            self.keep(*h);
        }
        self.map.get_mut(h)
    }

    pub fn insert(&mut self, h: Handle, o: Object) -> Option<Object> {
        self.keep(h);
        self.map.insert(h, o)
    }

    pub fn remove(&mut self, h: &Handle) -> Option<Object> {
        if self.map.contains_key(h) {
            self.keep(*h);
        }
        self.map.remove(h)
    }

    /// Every object, to change: while journaling, each is kept first.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut Object> {
        if self.journal.is_some() {
            let all: Vec<Handle> = self.map.keys().copied().collect();
            for h in all {
                self.keep(h);
            }
        }
        self.map.values_mut()
    }

    /// Every object with its handle, to change: as for `values_mut`.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Handle, &mut Object)> {
        if self.journal.is_some() {
            let all: Vec<Handle> = self.map.keys().copied().collect();
            for h in all {
                self.keep(h);
            }
        }
        self.map.iter_mut()
    }

    pub fn retain(&mut self, mut f: impl FnMut(&Handle, &mut Object) -> bool) {
        let gone: Vec<Handle> = self
            .map
            .iter_mut()
            .filter_map(|(h, o)| (!f(h, o)).then_some(*h))
            .collect();
        for h in gone {
            self.remove(&h);
        }
    }

    pub fn clear(&mut self) {
        let all: Vec<Handle> = self.map.keys().copied().collect();
        for h in all {
            self.remove(&h);
        }
    }

    pub fn into_map(self) -> BTreeMap<Handle, Object> {
        self.map
    }

    /// The entity list of block record `block`, if it is one.
    fn list(&mut self, block: Handle) -> Option<&mut Vec<Handle>> {
        match self.map.get_mut(&block).map(|o| &mut o.kind) {
            Some(ObjectKind::BlockHeader(b)) => Some(&mut b.entities),
            _ => None,
        }
    }

    fn record(&mut self, block: Handle, op: ListOp) {
        if let Some(j) = &mut self.journal {
            let t = j.touched.entry(block).or_default();
            // Once the record is kept whole, it undoes this too.
            if t.prior.is_none() {
                t.list_ops.push(op);
            }
        }
    }

    /// Put `h` in block record `block`'s entity list at `at` (the end when
    /// `None`). Nothing when `block` is not a block record.
    pub fn list_insert(&mut self, block: Handle, at: Option<usize>, h: Handle) {
        let Some(list) = self.list(block) else {
            return;
        };
        let at = at.unwrap_or(list.len()).min(list.len());
        list.insert(at, h);
        self.record(block, ListOp::Insert(at, h));
    }

    /// Take `h` out of block record `block`'s entity list, wherever it is.
    pub fn list_remove(&mut self, block: Handle, h: Handle) {
        let Some(list) = self.list(block) else {
            return;
        };
        let Some(at) = list.iter().position(|x| *x == h) else {
            return;
        };
        list.remove(at);
        self.record(block, ListOp::Remove(at, h));
        // Listed twice (a damaged file): take the rest out too.
        while let Some(list) = self.list(block) {
            let Some(at) = list.iter().position(|x| *x == h) else {
                break;
            };
            list.remove(at);
            self.record(block, ListOp::Remove(at, h));
        }
    }

    fn undo_op(&mut self, block: Handle, op: ListOp) {
        let Some(list) = self.list(block) else {
            return;
        };
        match op {
            ListOp::Insert(at, h) => {
                if list.get(at) == Some(&h) {
                    list.remove(at);
                } else if let Some(i) = list.iter().position(|x| *x == h) {
                    list.remove(i);
                }
            }
            ListOp::Remove(at, h) => list.insert(at.min(list.len()), h),
        }
    }

    /// Take back what `j` records (see [`Touch`]); the record that makes
    /// the same change again. Nothing is copied: objects move between the
    /// map and the records.
    pub fn revert(&mut self, j: Journal) -> Journal {
        debug_assert!(self.journal.is_none(), "revert while journaling");
        let mut back = Journal::default();
        for (h, t) in j.touched {
            let mut inv = Touch::default();
            if let Some(prior) = t.prior {
                inv.prior = Some(self.map.remove(&h));
                if let Some(o) = prior {
                    self.map.insert(h, o);
                }
            }
            for op in t.list_ops.iter().rev() {
                self.undo_op(h, *op);
                // With the object put back whole, it needs no list changes.
                if inv.prior.is_none() {
                    inv.list_ops.push(op.inverse());
                }
            }
            back.touched.insert(h, inv);
        }
        back
    }
}

impl Deref for ObjectMap {
    type Target = BTreeMap<Handle, Object>;
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

/// A copy has no journal: it is a new drawing.
impl Clone for ObjectMap {
    fn clone(&self) -> ObjectMap {
        ObjectMap {
            map: self.map.clone(),
            journal: None,
        }
    }
}

/// The objects only; an open journal is not part of the drawing.
impl PartialEq for ObjectMap {
    fn eq(&self, other: &ObjectMap) -> bool {
        self.map == other.map
    }
}

impl std::fmt::Debug for ObjectMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.map.fmt(f)
    }
}

impl From<BTreeMap<Handle, Object>> for ObjectMap {
    fn from(map: BTreeMap<Handle, Object>) -> ObjectMap {
        ObjectMap { map, journal: None }
    }
}

impl FromIterator<(Handle, Object)> for ObjectMap {
    fn from_iter<I: IntoIterator<Item = (Handle, Object)>>(it: I) -> ObjectMap {
        ObjectMap::from(it.into_iter().collect::<BTreeMap<_, _>>())
    }
}

impl Extend<(Handle, Object)> for ObjectMap {
    fn extend<I: IntoIterator<Item = (Handle, Object)>>(&mut self, it: I) {
        for (h, o) in it {
            self.insert(h, o);
        }
    }
}

impl IntoIterator for ObjectMap {
    type Item = (Handle, Object);
    type IntoIter = std::collections::btree_map::IntoIter<Handle, Object>;
    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl<'a> IntoIterator for &'a ObjectMap {
    type Item = (&'a Handle, &'a Object);
    type IntoIter = std::collections::btree_map::Iter<'a, Handle, Object>;
    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}
