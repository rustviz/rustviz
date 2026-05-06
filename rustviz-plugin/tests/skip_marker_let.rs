// `// rustviz: skip` on a `let` keeps the variable's RAP
// registered (so any non-skipped reference still resolves) but
// drops every event that touches it. The result is no timeline
// column for `q` and no events naming `q`.

fn main() {
    let s = String::from("kept");
    let q = String::from("skipped"); // rustviz: skip
    println!("{} {}", s, q);
}
