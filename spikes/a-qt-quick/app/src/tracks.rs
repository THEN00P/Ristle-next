//! The track table model. Rows are seeds; strings are built on demand.

use crate::models::qobject;


use crate::catalog::{self, Rng};
use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant};
use qobject::TrackRole;

pub struct TrackModelRust {
    pub(crate) rows: i32,
    seeds: Vec<u32>,
    next_seed: u32,
    rng: Rng,
    pacer: catalog::Pacer,
}

impl Default for TrackModelRust {
    fn default() -> Self {
        Self { rows: 0, seeds: Vec::new(), next_seed: 0, rng: Rng::new(0x2545_f491), pacer: Default::default() }
    }
}

impl qobject::TrackModel {
    pub(crate) fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(&seed) = self.seeds.get(index.row() as usize) else {
            return QVariant::default();
        };
        match (TrackRole { repr: role }) {
            TrackRole::Number => QVariant::from(&(index.row() + 1)),
            TrackRole::Title => QVariant::from(&QString::from(catalog::title(seed).as_str())),
            TrackRole::Artist => QVariant::from(&QString::from(catalog::artist(seed))),
            TrackRole::Album => QVariant::from(&QString::from(catalog::album(seed).as_str())),
            TrackRole::Added => QVariant::from(&QString::from(catalog::added(seed).as_str())),
            TrackRole::Duration => {
                QVariant::from(&QString::from(catalog::duration(seed).as_str()))
            }
            TrackRole::Art => QVariant::from(&catalog::art(seed)),
            TrackRole::Source => QVariant::from(&catalog::source(seed)),
            TrackRole::Liked => QVariant::from(&catalog::liked(seed)),
            TrackRole::Seed => QVariant::from(&(seed as i32)),
            _ => QVariant::default(),
        }
    }

    pub(crate) fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut roles = QHash::<QHashPair_i32_QByteArray>::default();
        for (role, name) in [
            (TrackRole::Number, "number"),
            (TrackRole::Title, "title"),
            (TrackRole::Artist, "artist"),
            (TrackRole::Album, "album"),
            (TrackRole::Added, "added"),
            (TrackRole::Duration, "duration"),
            (TrackRole::Art, "art"),
            (TrackRole::Source, "source"),
            (TrackRole::Liked, "liked"),
            (TrackRole::Seed, "seed"),
        ] {
            roles.insert(role.repr, QByteArray::from(name));
        }
        roles
    }

    pub(crate) fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.seeds.len() as i32
    }

    pub(crate) fn set_rows(mut self: Pin<&mut Self>, rows: i32) {
        let rows = rows.max(0);
        unsafe {
            self.as_mut().begin_reset_model();
            {
                let mut this = self.as_mut().rust_mut();
                this.seeds = (0..rows as u32).collect();
                this.next_seed = rows as u32;
                this.rows = rows;
            }
            self.as_mut().end_reset_model();
        }
        self.rows_changed();
    }

    pub(crate) fn churn_at(mut self: Pin<&mut Self>, rate: i32, around: i32) {
        let ops = self.as_mut().rust_mut().pacer.due(rate as f64);
        self.churn(ops as i32, around);
    }

    pub(crate) fn churn(mut self: Pin<&mut Self>, ops: i32, around: i32) {
        let root = QModelIndex::default();
        for _ in 0..ops {
            let len = self.seeds.len() as i32;
            if len < 2 {
                return;
            }
            let lo = (around - 20).clamp(0, len - 1);
            let hi = (around + 20).clamp(lo + 1, len);
            let (kind, a, b) = {
                let mut this = self.as_mut().rust_mut();
                let kind = this.rng.below(3);
                let a = lo + this.rng.below((hi - lo) as u32) as i32;
                let b = lo + this.rng.below((hi - lo) as u32) as i32;
                (kind, a, b)
            };
            unsafe {
                match kind {
                    0 => {
                        self.as_mut().begin_insert_rows(&root, a, a);
                        {
                            let mut this = self.as_mut().rust_mut();
                            let seed = this.next_seed;
                            this.next_seed += 1;
                            this.seeds.insert(a as usize, seed);
                        }
                        self.as_mut().end_insert_rows();
                    }
                    1 => {
                        self.as_mut().begin_remove_rows(&root, a, a);
                        self.as_mut().rust_mut().seeds.remove(a as usize);
                        self.as_mut().end_remove_rows();
                    }
                    _ => {
                        if a == b {
                            continue;
                        }
                        // Qt's destination is the row index before the move.
                        let dest = if b > a { b + 1 } else { b };
                        if self.as_mut().begin_move_rows(&root, a, a, &root, dest) {
                            {
                                let mut this = self.as_mut().rust_mut();
                                let seed = this.seeds.remove(a as usize);
                                this.seeds.insert(b as usize, seed);
                            }
                            self.as_mut().end_move_rows();
                        }
                    }
                }
            }
        }
    }
}
