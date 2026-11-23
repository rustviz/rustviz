// #1300, #1307, #1299, #1312: the arrows, arrowheads and function logos of
// one row each get a spot of their own. Arrows on one row along the same
// stretch move to a track half a row off it; a call's logo on another
// arrow goes to its dot's other side; results of several calls handed to
// the caller stack in the gutter; arrows dipping below a line take their
// tracks narrowest first, so they don't cross.
fn show(s: String) {
    println!("{}", s);
}

fn make() -> String {
    String::from("x")
}

struct P {
    a: String,
    b: String,
}

// #1300: two borrows end on line 27 (r gives t back, d gives v back).
fn two_loans_end() {
    let v = vec![String::from("a")];
    let t = String::from("t");
    for d in &v {
        let r = &t;
        if d.len() > 1 {
            println!("{}", r);
            break;
        }
        println!("{}", r);
    }
}

// #1307: a call's logo on the arrow into its argument.
fn match_arm_call() {
    let opt = Some(String::from("hi"));
    match opt {
        Some(s) => show(s),
        None => {}
    }
}

fn if_let_call() {
    let opt = Some(String::from("hi"));
    if let Some(s) = opt { show(s) }
}

fn for_by_value() {
    let v = vec![String::from("a"), String::from("b")];
    for s in v {
        println!("{}", s);
    }
}

fn collect_returned(v: Vec<String>) -> Vec<String> {
    v.into_iter().filter(|s| !s.is_empty()).collect()
}

// #1299: several calls' results handed to the caller on one row.
fn pair() -> (String, String) {
    (make(), String::from("b"))
}

fn triple() -> (String, String, String) {
    (make(), String::from("b"), String::new())
}

fn array() -> [String; 2] {
    [make(), String::from("b")]
}

fn literal() -> P {
    P { a: make(), b: String::from("b") }
}

// #1312: the two arrows dipping below line 81 don't cross.
fn get_match() {
    let v = vec![1, 2, 3, 4, 5];
    let third: Option<&i32> = v.get(2);
    match third {
        Some(third) => println!("The third element is {third}"),
        None => println!("There is no third element."),
    }
}

fn main() {
    two_loans_end();
    match_arm_call();
    if_let_call();
    for_by_value();
    collect_returned(vec![]);
    pair();
    triple();
    array();
    literal();
    get_match();
}
