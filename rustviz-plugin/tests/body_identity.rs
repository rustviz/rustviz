struct Pair(String, String);
mod left { pub fn take(p: super::Pair) { drop(p); } } mod right { pub fn take(p: super::Pair) {
    let first = p.0;
    drop(first);
    let length = p.1.len();
    let _ = length;
} }
