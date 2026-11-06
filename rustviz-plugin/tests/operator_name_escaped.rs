// An operator named `&` (`&a & &b` on HashSets is `BitAnd::bitand`)
// must be escaped in tooltips, or the SVG is not valid XML (#376).
use std::collections::HashSet;
fn main() {
    let a: HashSet<i32> = [1, 2].into_iter().collect();
    let b: HashSet<i32> = [2, 3].into_iter().collect();
    let c = &a & &b;
    println!("{:?}", c);
}
