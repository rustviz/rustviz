macro_rules! twice { ($p:pat) => { let $p = String::new(); let $p = 7usize; }; }
fn expansions() { twice!(value); }

fn captures(x: String) {
    let f = move || { let x = x; let x = x.len(); drop(x); };
    f();
}
