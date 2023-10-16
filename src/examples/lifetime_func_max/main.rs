/* --- BEGIN Variable Definitions ---
LifetimeVars a; LifetimeVars b; LifetimeVars r; LifetimeVars x; LifetimeVars y;
--- END Variable Definitions --- */
fn main(){
    let a = 10;
    let b = 6;
    let r: &i32;
    {
        let x: &i32 = &a;
        let y: &i32 = &b;
        r = max(x,y); // !{ Lifetime(<FUNC: max>[x{6:8}*CRPT* bind to variable a *DRPT* dropped after the last use of x][y{7:8}*CRPT* bind to variable b *DRPT* dropped after the last use of y]->[r{4:10}*CRPT* created as reference to i32 type *BODY* can bind to either a or b during runtime *DRPT* dropped as last used by println (also main exits)] )}
    }
    println!("r is {}",r);
}

fn max<'a>(x: &'a i32, y: &'a i32) -> &'a i32{
    if x >= y{
        x
    }
    else{
        y
    }
}