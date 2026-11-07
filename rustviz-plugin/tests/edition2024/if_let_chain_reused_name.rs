// #696: the chain's binding `a` reuses the name of the first `if let`'s
// binding, whose scope ended at its `}`. The chain touches no variable
// declared outside it, but it was refused ("`if let` chains that assign
// to a variable declared outside the condition (`a`, line 12)"): every
// variable seen so far counted as outside the condition, even one no
// longer in scope.

pub fn f(n: i32, o: Option<String>, p: Option<String>) {
    if let Some(a) = o {
        println!("{a}");
    }
    if let Some(a) = p && n > 0 {
        println!("{a}");
    } else {
        println!("no");
    }
}
