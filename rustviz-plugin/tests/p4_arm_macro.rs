macro_rules! guarded {
    ($p:pat) => {
        match std::hint::black_box(Some(0)) { $p if std::hint::black_box(true) => 1, $p => 2, _ => 3 }
    };
}
fn macro_guard() -> u8 { guarded!(Some(_)) }
