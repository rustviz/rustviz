/* --- BEGIN Variable Definitions ---
Owner x; Owner y; Owner k;
--- END Variable Definitions --- */
fn main() {
    let x = 5; // !{ Bind(x) }
    let y = x; // !{ Copy(x->y) }
    let k; // !{ Bind(k) }
    k = max(x,y); // !{ Lifetime(<FUNC: max>[x{3:15}*NAME* wow wow *DRPT* bow bow][y{7:15}]->[k{8:30}]) }
} /* !{
    GoOutOfScope(x),
    GoOutOfScope(y)
} */