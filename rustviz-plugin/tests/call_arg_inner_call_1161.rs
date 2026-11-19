// #1161: a std function called on another call's result
// (`std::process::exit(run(config))`, `drop(tag(s, ..))`). The inner call
// takes the variables; the outer one gets the value it returns. An old
// `println!` decoder drew the inner call's arguments as passed to the
// outer function ("Move from config to std::process::exit"), or nothing.
struct Config { name: String }
fn run(c: Config) -> i32 {
    println!("{}", c.name);
    0
}
fn tag(name: String, xs: &[i32]) -> String {
    let _ = xs;
    name
}
fn main() {
    let config = Config { name: String::from("app") };
    let s = String::from("s");
    let n = 1;
    drop(tag(s, &[n, 2]));
    std::process::exit(run(config));
}
