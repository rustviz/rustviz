use std::fmt;

#[derive(Debug)]
enum Token { Label(&'static str) }
impl Drop for Token {
    fn drop(&mut self) { let Self::Label(label) = self; println!("drop {label}"); }
}
impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { let Self::Label(label) = self; f.write_str(label) }
}
use Token::Label as token;
fn repeated(mut a: Token, mut b: Token, c: Token) {
    let mut d = token("untouched");
    (a, a, b, d) = (b, c, a, token("new d"));
    println!("stored {a} {b} {d}");
}

fn refill(c: bool, d: bool) {
    let mut x = token("initial x");
    drop(x);
    x = if c {
        if d { x = token("refilled x"); }
        token("then x")
    } else {
        token("else x")
    };
    println!("stored {x}");
}

fn box_arms(take: bool, choose: bool, left: Box<String>, right: Box<String>) {
    let mut boxed = Box::new(String::from("old contents"));
    if take { drop(*boxed); }
    boxed = if choose { left } else { right };
    println!("stored {boxed}");
}

fn empty_box_arms(choose: bool, first: Box<String>, second: Box<String>) {
    let mut only_allocation = Box::new(String::from("old contents"));
    drop(*only_allocation);
    only_allocation = if choose { first } else { second };
    println!("stored {only_allocation}");
}

fn array_payload(take: bool, mut boxed_array: Box<[Token; 2]>) {
    if take {
        let [first, _] = *boxed_array;
        drop(first);
    }
    *boxed_array = [token("new first"), token("new second")];
    println!("stored {}", boxed_array[0]);
}

struct Pair { left: Token, right: Token }
fn refill_field(refill: bool, mut pair: Pair, next: Pair) {
    drop(pair.left);
    if refill { pair.left = token("refilled left"); }
    pair = next;
    println!("stored {} {}", pair.left, pair.right);
}

fn exhausted_array(mut exhausted: [Token; 2], next: [Token; 2]) {
    let [first, second] = exhausted;
    exhausted = next;
    println!("moved {first} {second}; stored {}", exhausted[0]);
}

fn self_block(exit: bool, mut original: Token, ticket: Token) {
    original = {
        if exit {
            drop(original);
            drop(ticket);
            return;
        }
        original
    };
    println!("stored {original}");
}

fn duplicate_guard(n: i32, mut guarded: String) {
    match n {
        0 | 1 if {
            guarded = String::from("new");
            !guarded.is_empty()
        } => println!("stored {guarded}"),
        _ => (),
    }
}

enum Member<'a> { Value(&'a str, Token) }
struct Record<'a> { member: Member<'a> }
fn same_projection<'a>(mut record: Record<'a>, incoming: Member<'a>) {
    let place = &mut record;
    place.member = incoming;
    let Member::Value(label, tracked) = &record.member;
    println!("stored {label} {tracked}");
}

fn main() {
    repeated(token("a"), token("b"), token("c"));
    refill(true, true);
    refill(true, false);
    array_payload(true, Box::new([token("old first"), token("old second")]));
    array_payload(false, Box::new([token("old first"), token("old second")]));
    refill_field(true, Pair { left: token("old left"), right: token("old right") },
        Pair { left: token("new left"), right: token("new right") });
    exhausted_array([token("old 0"), token("old 1")], [token("new 0"), token("new 1")]);
    self_block(true, token("self"), token("ticket"));
    self_block(false, token("self"), token("ticket"));
    same_projection(Record { member: Member::Value("old", token("old member")) },
        Member::Value("new", token("new member")));
}
