//! Module de test : compte ses battements, range le compte, panique sur
//! demande (action 99). Son etiquette (`CP_FIXTURE_TAG`, fixee a la
//! compilation) dit quelle construction tourne.

use cp_sys::module::{HostRef, Module};

const TAG: &str = match option_env!("CP_FIXTURE_TAG") {
    Some(t) => t,
    None => "?",
};

struct Fixture {
    host: HostRef,
    n: u64,
}

impl Module for Fixture {
    fn init(host: HostRef, state: &[u8]) -> Self {
        let n = state.try_into().map(u64::from_le_bytes).unwrap_or(0);
        host.info(&format!("init {TAG} gen {} n {n}", host.generation()));
        Fixture { host, n }
    }

    fn shutdown(&mut self) {
        self.host.store_state(&self.n.to_le_bytes());
        self.host.info(&format!("shutdown {TAG} {}", self.n));
    }

    fn tick(&mut self) {
        self.n += 1;
        self.host.info(&format!("tick {TAG} {}", self.n));
    }

    fn on_action(&mut self, cmd: i32) -> bool {
        if cmd == 99 {
            panic!("panne voulue");
        }
        false
    }
}

cp_sys::export_module!(Fixture);
