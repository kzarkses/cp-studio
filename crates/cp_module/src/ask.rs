//! Les questions a l'utilisateur sans boite de Windows : un menu, un nom,
//! une confirmation s'ouvrent en popover dans la vue ([`cp_ui::pop`]).
//!
//! Un menu de Windows tenait le fil jusqu'au choix : le code d'un geste
//! demandait, recevait la reponse et continuait. Un popover rend la main
//! tout de suite. Plutot que de couper chaque geste en deux, le geste qui a
//! pose la question se REJOUE quand la reponse arrive, la reponse en main :
//! le meme code repasse, chaque question trouve sa reponse dans l'ordre, et
//! la suite s'execute. Une question de plus (un nom apres un menu) ouvre un
//! autre popover et le geste se rejouera avec les deux reponses.
//!
//! Le code avant une question ne doit donc rien changer (il repassera) :
//! lire l'etat, construire le menu. Un choix de menu se rejoue par son
//! texte autant que par son numero : si la liste a change entre-temps (un
//! preset ajoute), rien ne se fait plutot qu'autre chose.

use std::collections::VecDeque;

use cp_sys::ui::MenuEntry;
use cp_ui::pop::Out;

/// Une reponse deja donnee.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Answer {
    /// Un choix de menu : son numero et son texte.
    Pick(u32, String),
    Text(String),
    Yes,
}

/// Ce que vaut une question au passage du code.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Ask<T> {
    /// La reponse (on rejoue).
    Ready(T),
    /// Pas encore de reponse : l'appelant ouvre le popover, et le geste
    /// s'arrete la (la reponse le rejouera).
    Open,
    /// Rien a demander ici (pas de geste a rejouer, ou un rejeu qui ne
    /// correspond plus) : le geste s'arrete.
    Skip,
}

struct Pending<A> {
    act: A,
    given: Vec<Answer>,
    entries: Vec<MenuEntry>,
}

/// Les questions d'une vue : le geste en cours et les reponses a rejouer.
pub(crate) struct Asker<A> {
    cur: Option<A>,
    answers: VecDeque<Answer>,
    given: Vec<Answer>,
    pending: Option<Pending<A>>,
}

impl<A> Default for Asker<A> {
    fn default() -> Self {
        Asker {
            cur: None,
            answers: VecDeque::new(),
            given: Vec::new(),
            pending: None,
        }
    }
}

impl<A: Clone> Asker<A> {
    /// Un geste commence. Vrai : c'est le plus exterieur (celui qu'une
    /// question rejouera) ; un geste fait par un autre geste n'en est pas un.
    pub fn begin(&mut self, a: &A) -> bool {
        if self.cur.is_some() {
            return false;
        }
        self.cur = Some(a.clone());
        self.given.clear();
        true
    }

    /// Le geste `begin` (vrai) se termine : ce qui reste a rejouer se perd.
    pub fn end(&mut self, top: bool) {
        if top {
            self.cur = None;
            self.answers.clear();
            self.given.clear();
        }
    }

    fn next(&mut self) -> Option<Answer> {
        self.answers.pop_front()
    }

    fn note(&mut self, entries: &[MenuEntry]) -> bool {
        match self.cur.clone() {
            Some(act) => {
                self.pending = Some(Pending {
                    act,
                    given: self.given.clone(),
                    entries: entries.to_vec(),
                });
                true
            }
            None => false,
        }
    }

    /// Un menu : le choix du rejeu, ou l'ouvrir.
    pub fn pick(&mut self, entries: &[MenuEntry]) -> Ask<u32> {
        if let Some(a) = self.next() {
            if let Answer::Pick(id, t) = &a {
                if MenuEntry::text_of(entries, *id).as_deref() == Some(t.as_str()) {
                    let id = *id;
                    self.given.push(a);
                    return Ask::Ready(id);
                }
            }
            self.answers.clear();
            return Ask::Skip;
        }
        if self.note(entries) {
            Ask::Open
        } else {
            Ask::Skip
        }
    }

    /// Un nom : celui du rejeu, ou ouvrir la saisie.
    pub fn text(&mut self) -> Ask<String> {
        if let Some(a) = self.next() {
            if let Answer::Text(t) = &a {
                let t = t.clone();
                self.given.push(a);
                return Ask::Ready(t);
            }
            self.answers.clear();
            return Ask::Skip;
        }
        if self.note(&[]) { Ask::Open } else { Ask::Skip }
    }

    /// Une confirmation : celle du rejeu, ou l'ouvrir.
    pub fn yes(&mut self) -> Ask<()> {
        if let Some(a) = self.next() {
            if a == Answer::Yes {
                self.given.push(a);
                return Ask::Ready(());
            }
            self.answers.clear();
            return Ask::Skip;
        }
        if self.note(&[]) { Ask::Open } else { Ask::Skip }
    }

    /// Le popover a rendu : le geste a rejouer (ses reponses sont pretes),
    /// ou rien (ferme, ou rien n'attendait).
    pub fn answer(&mut self, out: &Out) -> Option<A> {
        let p = self.pending.take()?;
        let ans = match out {
            Out::Pick(id) => Answer::Pick(*id, MenuEntry::text_of(&p.entries, *id)?),
            Out::Text(t) => Answer::Text(t.clone()),
            Out::Yes => Answer::Yes,
            _ => return None,
        };
        let mut q = p.given;
        q.push(ans);
        self.answers = q.into();
        Some(p.act)
    }

    /// Une question attend sa reponse.
    #[cfg(test)]
    pub fn waiting(&self) -> bool {
        self.pending.is_some()
    }

    /// La question est abandonnee (le popover s'est ferme).
    pub fn cancel(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum G {
        PresetMenu,
    }

    fn menu() -> Vec<MenuEntry> {
        vec![MenuEntry::item(1, "Init"), MenuEntry::item(3, "Save as…")]
    }

    /// Le geste de l'exemple : un menu, puis un nom pour « Save as… ».
    fn run(a: &mut Asker<G>, log: &mut Vec<String>) -> (bool, bool) {
        let top = a.begin(&G::PresetMenu);
        let (mut opened_menu, mut opened_name) = (false, false);
        match a.pick(&menu()) {
            Ask::Ready(3) => match a.text() {
                Ask::Ready(n) => log.push(format!("save {n}")),
                Ask::Open => opened_name = true,
                Ask::Skip => {}
            },
            Ask::Ready(_) => log.push("other".into()),
            Ask::Open => opened_menu = true,
            Ask::Skip => {}
        }
        a.end(top);
        (opened_menu, opened_name)
    }

    #[test]
    fn a_gesture_replays_with_its_answers_in_order() {
        let mut a = Asker::default();
        let mut log = Vec::new();
        assert_eq!(run(&mut a, &mut log), (true, false));
        // « Save as… » choisi : le geste se rejoue et demande le nom.
        assert_eq!(a.answer(&Out::Pick(3)), Some(G::PresetMenu));
        assert_eq!(run(&mut a, &mut log), (false, true));
        assert!(log.is_empty(), "rien ne s'est fait avant le nom");
        // Le nom : le geste se rejoue avec les deux reponses, et sauve.
        assert_eq!(a.answer(&Out::Text("Night".into())), Some(G::PresetMenu));
        assert_eq!(run(&mut a, &mut log), (false, false));
        assert_eq!(log, vec!["save Night".to_string()]);
        assert!(!a.waiting());
    }

    #[test]
    fn a_closed_popover_or_a_changed_menu_does_nothing() {
        let mut a = Asker::default();
        let mut log = Vec::new();
        run(&mut a, &mut log);
        a.cancel();
        assert_eq!(a.answer(&Out::Pick(3)), None);
        // Un choix dont le texte a change au rejeu : rien.
        let top = a.begin(&G::PresetMenu);
        assert_eq!(a.pick(&menu()), Ask::Open);
        a.end(top);
        a.answer(&Out::Pick(1));
        let top = a.begin(&G::PresetMenu);
        let other = vec![MenuEntry::item(1, "Rename…")];
        assert_eq!(a.pick(&other), Ask::Skip);
        a.end(top);
        // Hors d'un geste, une question ne s'ouvre pas.
        let mut b: Asker<G> = Asker::default();
        assert_eq!(b.text(), Ask::Skip);
    }
}
