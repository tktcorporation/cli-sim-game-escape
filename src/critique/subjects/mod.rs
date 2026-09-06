//! 評価対象ゲームのアダプタ。

mod cookie;
mod everlight;
mod expedition;
mod factory;
mod loopmarch;
mod pachinko;

use crate::critique::probe::Subject;

pub fn all() -> Vec<Box<dyn Subject>> {
    vec![
        cookie(),
        everlight(),
        pachinko(),
        loopmarch(),
        factory(),
        expedition(),
    ]
}

pub fn cookie() -> Box<dyn Subject> {
    Box::new(cookie::CookieSubject::new())
}

pub fn everlight() -> Box<dyn Subject> {
    Box::new(everlight::EverlightSubject::new())
}

pub fn pachinko() -> Box<dyn Subject> {
    Box::new(pachinko::PachinkoSubject::new())
}

pub fn loopmarch() -> Box<dyn Subject> {
    Box::new(loopmarch::LoopMarchSubject::new())
}

pub fn factory() -> Box<dyn Subject> {
    Box::new(factory::FactorySubject::new())
}

pub fn expedition() -> Box<dyn Subject> {
    Box::new(expedition::ExpeditionSubject::new())
}
