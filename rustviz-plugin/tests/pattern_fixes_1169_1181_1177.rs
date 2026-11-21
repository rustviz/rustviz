// `_ = s;` binds nothing, so it moves nothing (#1169), unlike a `_` that
// matches a part of a tuple literal, which the literal moves.
// A parameter pattern that leaves parts of the argument unbound (`..`,
// `_`): those are dropped when the function returns (#1181).
// `@` bindings: the names inside the sub-pattern are bound too (#1177).
struct Person { name: String, bio: String }
struct W(String, String, i32);
struct In { tag: String, n: i32 }
struct Out { inner: In, label: String }

fn wild_assign() {
    let s = String::from("a");
    _ = s;
    println!("{}", s);
    let p = Person { name: String::from("n"), bio: String::from("b") };
    _ = p.name;
    println!("{}", p.name);
    let t = String::from("t");
    let u = String::from("u");
    let a;
    (a, _) = (t, u);
    println!("{}", a);
}

fn greet(Person { name, .. }: Person) {
    println!("{}", name);
}

fn first((a, _): (String, String)) -> String {
    a
}

fn named(Person { name: nm, bio: _ }: Person) {
    println!("{}", nm);
}

fn tuple_struct(W(w0, ..): W) {
    println!("{}", w0);
}

fn nested(Out { inner: In { n, .. }, label }: Out) {
    println!("{} {}", n, label);
}

fn borrowed(&(ref b0, _): &(String, String)) {
    println!("{}", b0);
}

fn at_bindings(o: Option<i32>) {
    if let whole @ Some(n) = o {
        println!("{:?} {}", whole, n);
    }
    match o {
        v @ Some(k) if k > 2 => println!("{:?} {}", v, k),
        _ => {}
    }
}

fn main() {
    wild_assign();
    greet(Person { name: String::from("n"), bio: String::from("b") });
    println!("{}", first((String::from("a"), String::from("b"))));
    named(Person { name: String::from("n"), bio: String::from("b") });
    tuple_struct(W(String::from("x"), String::from("y"), 1));
    nested(Out { inner: In { tag: String::from("t"), n: 1 }, label: String::from("l") });
    borrowed(&(String::from("a"), String::from("b")));
    at_bindings(Some(3));
}
