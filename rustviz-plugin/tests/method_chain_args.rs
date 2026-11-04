// Arguments of later calls in a method chain (#132). The receiver of
// `push` / `unwrap_or` / `contains` is a temporary, but the arguments
// are ordinary values: `n` and `default` are moved into the call, and
// `needle` is borrowed by it.

fn main() {
    let mut names: Vec<Vec<String>> = vec![Vec::new()];
    let n = String::from("ann");
    names.last_mut().unwrap().push(n);

    let mut opt: Option<String> = Some(String::from("a"));
    let default = String::from("d");
    let x = opt.take().unwrap_or(default);

    let text = String::from(" a b ");
    let needle = String::from("a");
    let found = text.trim().contains(&needle);
    println!("{:?} {} {:?} {} {}", names, x, opt, found, needle);
}
