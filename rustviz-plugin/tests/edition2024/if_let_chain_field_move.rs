// #281: the condition may move `p.name` (into `a`) before
// `a.len() > 5` fails, so `p`'s scope end drops `p.name` only if
// nothing above moved it (the condition, not a branch, may have).

struct P { name: Option<String>, id: String }

fn main() {
    let p = P { name: Some(String::from("a")), id: String::from("i") };
    if let Some(a) = p.name && a.len() > 5 {
        println!("{}", a);
    } else {
        println!("short");
    }
    println!("{}", p.id);
}
