// Returning one Copy element or field (#1052) copies only that part to
// the caller; the array, Vec or static it comes from keeps all of its
// contents. `first_of` returns a copy of an i32, not a reference into
// v, so v's borrow ends in the function.
struct Pt { x: i32, y: i32 }
static PRIMES: [i32; 3] = [2, 3, 5];
static ORIGIN: Pt = Pt { x: 0, y: 0 };

fn first() -> i32 {
    let v = [1, 2];
    v[0]
}

fn corner() -> i32 {
    let g = [[1, 2], [3, 4]];
    g[1][0]
}

fn first_of(v: &Vec<i32>) -> i32 {
    v[0]
}

fn pick(c: bool) -> i32 {
    let w = vec![1, 2];
    if c {
        return w[0];
    }
    w[1]
}

fn first_prime() -> i32 {
    PRIMES[0]
}

fn origin_x() -> i32 {
    ORIGIN.x
}

fn main() {
    let v = vec![7];
    println!("{} {} {} {} {} {} {}", first(), corner(), first_of(&v), pick(true), first_prime(), origin_x(), ORIGIN.y);
}
