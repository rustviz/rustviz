/* --- BEGIN Variable Definitions ---
Owner six;
Owner x;
Function plus_one()
--- END Variable Definitions --- */
fn main() {
    let six = plus_one(5); // !{ Copy(None->plus_one()), Copy(plus_one()->six) }
} // !{ GoOutOfScope(six) }

fn plus_one(x: i32) -> i32 { // !{ InitOwnerParam(x) }
    x + 1 // !{ Copy(x->None) }
} // !{ GoOutOfScope(x) }