fn pick<'a, 'b: 'a>(x: &'a str, y: &'b str) -> &'a str { if x.len() > 3 { x } else { y } }
fn first(a: String, b: String, c: bool) -> String { if c { a } else { b } }
fn by_match(o: Option<String>) -> usize { match o { Some(s) => s.len(), None => 0 } }
fn if_only(s: String, c: bool) { if c { drop(s) } }
fn if_let(o: Option<String>) -> usize { if let Some(s) = o { s.len() } else { 0 } }

fn let_if(a: String, b: String, c: bool) { let x = if c { a } else { b }; println!("{}", x); }
struct P { a: String, b: String }
fn fields(p: P, c: bool) -> String { if c { p.a } else { p.b } }
fn main() {
    pick("a", "b");
    first(String::new(), String::new(), true);
    by_match(None);
    if_only(String::new(), true);
    if_let(None);
    let_if(String::new(), String::new(), true);
    fields(P { a: String::new(), b: String::new() }, true);
}
