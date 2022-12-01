/* --- BEGIN Variable Definitions ---
Owner nums;
Owner nth;
Function println!();
Function vec!();
--- END Variable Definitions --- */
fn main(){
    let nums = vec![1,2,3]; // !{ Move(vec!()->nums) }
    
    let nth = |x| nums[x]; // !{ StaticBorrow(nums->nth) }
    
    println!("{}",nth(1)); // !{StaticDie(nth->nums)}
    
    println!("{}",nums[1]);
}// !{ GoOutOfScope(nums) , GoOutOfScope(nth) } 