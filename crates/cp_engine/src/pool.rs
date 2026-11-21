//! Le vivier : les clips resident en RAM, decodes une fois (ancien `cp_pool`).
//!
//! Il vit sur le fil principal. Le fil audio ne le voit jamais : il recoit un
//! `Arc<ClipData>` dans la commande qui lance une voix.
//!
//! Retirer, c'est demander : un clip retire ne peut plus etre LANCE, mais il
//! reste en memoire tant qu'une voix le tient. `collect` ne libere que les clips
//! que plus personne ne tient (`Arc::strong_count == 1`), ce qui fait aussi que
//! la derniere reference n'est jamais lachee par le fil audio. L'ancien C++
//! deduisait les references d'un balayage des voix ; ici le compte est exact par
//! construction.

use std::sync::Arc;

use crate::types::{Frame, MAX_CHANS, MAX_CLIP_FRAMES, MAX_CLIPS};

/// La matiere d'un clip : entrelacee, `nch` canaux, au taux `srate`.
#[derive(Debug)]
pub struct ClipData {
    samples: Box<[f32]>,
    frames: Frame,
    nch: usize,
    srate: f64,
    id: i32,
}

impl ClipData {
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }
    pub fn frames(&self) -> Frame {
        self.frames
    }
    pub fn nch(&self) -> usize {
        self.nch
    }
    pub fn srate(&self) -> f64 {
        self.srate
    }
    /// L'emplacement du vivier.
    pub fn id(&self) -> i32 {
        self.id
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PoolError {
    Full,
    TooLong,
    BadChannels,
    Empty,
    NotWholeFrames,
}

struct Slot {
    clip: Arc<ClipData>,
    retire_wanted: bool,
}

pub struct Pool {
    slots: Vec<Option<Slot>>,
    bytes: usize,
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

impl Pool {
    pub fn new() -> Pool {
        Pool {
            slots: (0..MAX_CLIPS).map(|_| None).collect(),
            bytes: 0,
        }
    }

    /// Range une matiere decodee. Rend l'emplacement.
    pub fn add(&mut self, samples: Vec<f32>, nch: usize, srate: f64) -> Result<i32, PoolError> {
        if !(1..=MAX_CHANS).contains(&nch) {
            return Err(PoolError::BadChannels);
        }
        if samples.is_empty() {
            return Err(PoolError::Empty);
        }
        if samples.len() % nch != 0 {
            return Err(PoolError::NotWholeFrames);
        }
        let frames = (samples.len() / nch) as Frame;
        if frames > MAX_CLIP_FRAMES {
            return Err(PoolError::TooLong);
        }
        let id = self
            .slots
            .iter()
            .position(Option::is_none)
            .ok_or(PoolError::Full)?;
        self.bytes += samples.len() * size_of::<f32>();
        self.slots[id] = Some(Slot {
            clip: Arc::new(ClipData {
                samples: samples.into_boxed_slice(),
                frames,
                nch,
                srate,
                id: id as i32,
            }),
            retire_wanted: false,
        });
        Ok(id as i32)
    }

    /// Un clip LANCABLE (pas retire).
    pub fn get(&self, id: i32) -> Option<&Arc<ClipData>> {
        let s = self.slots.get(usize::try_from(id).ok()?)?.as_ref()?;
        (!s.retire_wanted).then_some(&s.clip)
    }

    /// Plus aucun lancement ; la matiere part des que plus personne ne la tient.
    pub fn retire(&mut self, id: i32) {
        if let Some(Some(s)) = usize::try_from(id).ok().and_then(|i| self.slots.get_mut(i)) {
            s.retire_wanted = true;
        }
    }

    /// Une voix (ou une commande en vol) tient-elle encore ce clip ?
    pub fn in_use(&self, id: i32) -> bool {
        usize::try_from(id)
            .ok()
            .and_then(|i| self.slots.get(i))
            .and_then(Option::as_ref)
            .is_some_and(|s| Arc::strong_count(&s.clip) > 1)
    }

    /// Libere les clips retires que plus personne ne tient. Fil principal.
    pub fn collect(&mut self) -> usize {
        let mut n = 0;
        for slot in &mut self.slots {
            let free = slot
                .as_ref()
                .is_some_and(|s| s.retire_wanted && Arc::strong_count(&s.clip) == 1);
            if free {
                if let Some(s) = slot.take() {
                    self.bytes -= s.clip.samples.len() * size_of::<f32>();
                }
                n += 1;
            }
        }
        n
    }

    pub fn loaded_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    pub fn bytes_resident(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_clip_retire_reste_tant_qu_on_le_tient() {
        let mut p = Pool::new();
        let id = p.add(vec![0.5; 100], 1, 48000.0).unwrap();
        let held = p.get(id).unwrap().clone();
        p.retire(id);
        assert!(p.get(id).is_none(), "plus lancable");
        assert!(p.in_use(id));
        assert_eq!(p.collect(), 0, "une voix le tient");
        assert_eq!(p.loaded_count(), 1);
        drop(held);
        assert_eq!(p.collect(), 1);
        assert_eq!(p.loaded_count(), 0);
        assert_eq!(p.bytes_resident(), 0);
    }

    #[test]
    fn bornes() {
        let mut p = Pool::new();
        assert_eq!(p.add(vec![], 1, 48000.0), Err(PoolError::Empty));
        assert_eq!(
            p.add(vec![0.0; 3], 2, 48000.0),
            Err(PoolError::NotWholeFrames)
        );
        assert_eq!(p.add(vec![0.0; 4], 3, 48000.0), Err(PoolError::BadChannels));
    }
}
