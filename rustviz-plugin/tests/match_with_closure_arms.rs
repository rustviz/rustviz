// Combination test: each match arm declares its own closure that
// captures `s`. Multi-arm match (Branch) + per-arm closures (each
// emits its own capture event). Probes whether
// `register_iflet_let_bindings`-style decl tracking generalises
// to closures inside Branch arms.

fn show(_s: &String) {} // rustviz: hide

fn main() {
    let s = String::from("hi");
    let n = 1; // rustviz: skip
    match n {
        0 => {
            let f = || show(&s);
            f();
        }
        _ => {
            let g = || show(&s);
            g();
        }
    }
}
