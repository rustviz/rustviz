pub fn keeps_payload(maybe: Option<String>, take: bool) {
    match maybe {
        Some(value) if take => drop(value),
        _ => (),
    }
}

pub fn known_some(maybe: Option<String>) {
    let Some(value) = maybe else { return };
    drop(value);
}

pub fn residual(maybe: Option<String>, take: bool, result: Option<()>) -> Option<()> {
    match maybe {
        Some(value) if take => drop(value),
        _ => (),
    }
    result?;
    Some(())
}
