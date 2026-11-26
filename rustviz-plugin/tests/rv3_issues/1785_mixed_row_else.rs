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
