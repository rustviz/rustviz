// #792: a function whose tail (or `return`) is a call's result, or a tuple
// / struct / array literal of call results, hands that value to the caller.
struct Pair {
    a: String,
    b: String,
}

fn make() -> String {
    String::from("m")
}

fn num() -> i32 {
    7
}

fn bare() -> String {
    make()
}

fn early(c: bool) -> String {
    if c {
        return make();
    }
    String::from("x")
}

fn pair() -> (String, String) {
    (make(), String::from("b"))
}

fn rec() -> Pair {
    Pair { a: make(), b: make() }
}

fn arr() -> [String; 2] {
    [make(), make()]
}

fn copy_call() -> i32 {
    num()
}

fn main() {
    let _a = bare();
    let _b = early(true);
    let _c = pair();
    let _d = rec();
    let _e = arr();
    let _f = copy_call();
}

// A unit-returning call as the tail hands nothing to the caller.
fn log(s: &String) {
    drop(s.len());
}

fn tail_unit(s: &String) {
    log(s)
}
