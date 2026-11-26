// Known bug, from the closed 2.x fix PRs (fix/r3-joins-1594-1597-1598-1601); kept as a RustViz 3 regression case.
// Issue(s): #1598 (see known_bugs/README.md for what main draws wrong).
// #1598: the `else if let` has no `else`, and it and the outer `if` end
// on the same `}` row, so its join is raised half a row above that row
// (#1294). m's borrow ends as the implicit `else` starts: that dot now
// sits on the raised join's level, beside it, and the leg to the join
// leaves from it. It was on the `}` row, below the join, and the leg left
// the arm's column above it, from a point with no dot.
fn run(m: &mut String, o: Option<String>, a: bool) {
    if a {
    } else if let Some(x) = o {
        let n = m.len() + x.len();
    }
    println!("x");
}

fn main() {}
