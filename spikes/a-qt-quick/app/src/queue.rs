//! The queue model. Every edit is logged into the ring buffer so the analysis
//! can replay the expected order and compare it with the captured frames.

use crate::models::qobject;


use crate::catalog::{self, Rng};
use crate::spike::ffi::spike_mark;
use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant};
use qobject::QueueRole;

const EV_QUEUE_INSERT: u32 = 13;
const EV_QUEUE_REMOVE: u32 = 14;
const EV_QUEUE_MOVE: u32 = 15;
const EV_QUEUE_RESET: u32 = 18;

#[derive(Clone, Copy)]
struct Item {
    id: u32,
    seed: u32,
}

pub struct QueueModelRust {
    pub(crate) length: i32,
    items: Vec<Item>,
    next_id: u32,
    rng: Rng,
}

impl Default for QueueModelRust {
    fn default() -> Self {
        Self { length: 0, items: Vec::new(), next_id: 1, rng: Rng::new(0x1b87_3593) }
    }
}

/// The corruption check reads these two colours back from captured frames
/// (left and right half of the strip). Red and green of the first carry the
/// id; blue and the whole second colour are checks, so a blended pixel
/// (fading or antialiased) decodes to a valid id about once in 2^32.
pub fn marker(id: u32) -> String {
    let r = id & 0xff;
    let g = (id >> 8) & 0xff;
    let b = (r * 7 + g * 13 + 0x5a) & 0xff;
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub fn marker_check(id: u32) -> String {
    let r = id & 0xff;
    let g = (id >> 8) & 0xff;
    let c0 = (r * 11 + g * 3 + 0xa5) & 0xff;
    let c1 = (r ^ 0x6c).wrapping_add(g * 29) & 0xff;
    let c2 = (r * 5 + g * 17 + 0x33) & 0xff;
    format!("#{c0:02x}{c1:02x}{c2:02x}")
}

impl qobject::QueueModel {
    pub(crate) fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(item) = self.items.get(index.row() as usize) else {
            return QVariant::default();
        };
        let seed = item.seed;
        match (QueueRole { repr: role }) {
            QueueRole::ItemId => QVariant::from(&(item.id as i32)),
            QueueRole::Title => QVariant::from(&QString::from(catalog::title(seed).as_str())),
            QueueRole::Artist => QVariant::from(&QString::from(catalog::artist(seed))),
            QueueRole::Duration => {
                QVariant::from(&QString::from(catalog::duration(seed).as_str()))
            }
            QueueRole::Art => QVariant::from(&catalog::art(seed)),
            QueueRole::Source => QVariant::from(&catalog::source(seed)),
            QueueRole::Marker => QVariant::from(&QString::from(marker(item.id).as_str())),
            QueueRole::MarkerCheck => QVariant::from(&QString::from(marker_check(item.id).as_str())),
            _ => QVariant::default(),
        }
    }

    pub(crate) fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut roles = QHash::<QHashPair_i32_QByteArray>::default();
        for (role, name) in [
            (QueueRole::ItemId, "itemId"),
            (QueueRole::Title, "title"),
            (QueueRole::Artist, "artist"),
            (QueueRole::Duration, "duration"),
            (QueueRole::Art, "art"),
            (QueueRole::Source, "source"),
            (QueueRole::Marker, "marker"),
            (QueueRole::MarkerCheck, "markerCheck"),
        ] {
            roles.insert(role.repr, QByteArray::from(name));
        }
        roles
    }

    pub(crate) fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.items.len() as i32
    }

    pub(crate) fn set_length(mut self: Pin<&mut Self>, length: i32) {
        let length = length.clamp(0, 4000);
        unsafe {
            self.as_mut().begin_reset_model();
            {
                let mut this = self.as_mut().rust_mut();
                this.items = (1..=length as u32).map(|id| Item { id, seed: 7000 + id }).collect();
                this.next_id = length as u32 + 1;
                this.length = length;
            }
            self.as_mut().end_reset_model();
        }
        spike_mark(EV_QUEUE_RESET, length as u32);
        self.length_changed();
    }

    pub(crate) fn insert_at(mut self: Pin<&mut Self>, index: i32) {
        let index = index.clamp(0, self.items.len() as i32);
        unsafe {
            self.as_mut().begin_insert_rows(&QModelIndex::default(), index, index);
            let id = {
                let mut this = self.as_mut().rust_mut();
                let id = this.next_id;
                this.next_id += 1;
                this.items.insert(index as usize, Item { id, seed: 7000 + id });
                id
            };
            self.as_mut().end_insert_rows();
            spike_mark(EV_QUEUE_INSERT, ((index as u32) << 16) | id);
        }
    }

    pub(crate) fn remove_at(mut self: Pin<&mut Self>, index: i32) {
        if index < 0 || index as usize >= self.items.len() {
            return;
        }
        unsafe {
            self.as_mut().begin_remove_rows(&QModelIndex::default(), index, index);
            let id = self.as_mut().rust_mut().items.remove(index as usize).id;
            self.as_mut().end_remove_rows();
            spike_mark(EV_QUEUE_REMOVE, ((index as u32) << 16) | id);
        }
    }

    pub(crate) fn move_item(mut self: Pin<&mut Self>, from: i32, to: i32) {
        let len = self.items.len() as i32;
        if from == to || from < 0 || to < 0 || from >= len || to >= len {
            return;
        }
        let root = QModelIndex::default();
        let dest = if to > from { to + 1 } else { to };
        unsafe {
            if self.as_mut().begin_move_rows(&root, from, from, &root, dest) {
                {
                    let mut this = self.as_mut().rust_mut();
                    let item = this.items.remove(from as usize);
                    this.items.insert(to as usize, item);
                }
                self.as_mut().end_move_rows();
                spike_mark(EV_QUEUE_MOVE, ((from as u32) << 16) | to as u32);
            }
        }
    }

    pub(crate) fn steps(mut self: Pin<&mut Self>, count: i32, window: i32) {
        for _ in 0..count {
            self.as_mut().step(window);
        }
    }

    pub(crate) fn step(mut self: Pin<&mut Self>, window: i32) {
        let len = self.items.len() as i32;
        let window = window.clamp(1, len.max(1));
        let (kind, a, b) = {
            let mut this = self.as_mut().rust_mut();
            // Keep the length stable: grow when short, shrink when long.
            let bias = if len < 30 { 0 } else if len > 50 { 1 } else { this.rng.below(3) };
            (bias, this.rng.below(window as u32) as i32, this.rng.below(window as u32) as i32)
        };
        match kind {
            0 => self.insert_at(a),
            1 => self.remove_at(a),
            _ => self.move_item(a, b),
        }
    }
}
