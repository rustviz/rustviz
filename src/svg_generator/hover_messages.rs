/* Event Dot messages: shows up when someone hovers over a dot */

// Add styling to string name with <span>
fn fmt_style(plain: &String) -> String {
    let span_begin = String::from(
        "&lt;span style=&quot;font-family: 'Source Code Pro',
        Consolas, 'Ubuntu Mono', Menlo, 'DejaVu Sans Mono',
        monospace, monospace !important;&quot;&gt;"
    );
    let span_end = "&lt;/span&gt;";

    span_begin + plain + span_end
}

/* The Event dot does not connect to any arrows, we typically follow the following format:
   ... happens
 */

/* 1   0 （0 is initialized by let 0 = &1)
       |
       *   the star event
   
   example: 
   fn calculate_length(s: &String) -> usize { // s is a reference to a String
       s.len()      // a REF_GO_OUT_OF_SCOPE event
   } // Here, s goes out of scope. But because it does not have ownership of what
     // it refers to, nothing happens.
*/
pub fn event_dot_ref_go_out_out_scope(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} goes out of scope",
        my_name_fmt
    )
}

// 0
// |
// *   the star event
//
pub fn event_dot_owner_go_out_out_scope(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} goes out of scope", //we shouldn't say "the resource is dropped"
        my_name_fmt              //because we don't distinguish if the resource
    )                             //was moved from the variable earlier.
}

//     0
//     
// f1  *   the star event
// example: 
// fn calculate_length(s: &String) -> usize { // here s is initialized to some value
//    /* something happens */
// }
pub fn event_dot_init_param(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} is initialized as the function argument",
        my_name_fmt
    )
}


/* The Event dot is the source of an arrow, we typically follow the following format: 
   ... to <Resource Owner 1>
 */

// 1   0
//     |
// o<--*   the star event
// |   |
pub fn event_dot_copy_to(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is copied",
        my_name_fmt
    )
}

// 1   0
//     |
// o<--*   the star event
// |
pub fn event_dot_move_to(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is moved",
        my_name_fmt
    )
}

// def fn(p):
//  p
//  |
//  *-->   the return event
//
pub fn event_dot_move_to_caller(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is moved to the caller",
        my_name_fmt
    )
}

// 1   0
//     |
// o<--*   the star event (&)
// |   |
pub fn event_dot_static_lend(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);

    format!(
        "{0}'s resource is immutably borrowed",
        my_name_fmt
    )
}

// 1   0
//     |
// o<--*   the star event (&mut)
// |
pub fn event_dot_mut_lend(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);

    format!(
        "{0}'s resource is mutably borrowed",
        my_name_fmt
    )
}

// 0   1
//     |
// o<--o
// |   |
// *-->o   the star event (&)
pub fn event_dot_static_return(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s mutable borrow ends",
        my_name_fmt
    )
}

// 0   1
//     |
// o<--o
// |
// *-->o   the star event (&mut)
pub fn event_dot_mut_return(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s immutable borrow ends",
        my_name_fmt
    )
}

/* The Event dot is the destination of an arrow, we typically follow the following format: 
   ... from <Resource Owner 1>
 */

// 1   0        1   0
// |            |   
// o-->*   or   o-->*     the star event
// |   |            |
pub fn event_dot_acquire(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} acquires ownership of a resource",
        my_name_fmt
    )
}

// 1   0        1   0
// |            |   
// o-->*   or   o-->*     the star event
// |   |            |
pub fn event_dot_copy_from(my_name: &String, target_name: &String) -> String {
    format!(
        "{0} is initialized by copy from {1}",
        my_name,
        target_name
    )
}

// 0   1
//     |
// *<--o   the star event (&mut)
// |   |
pub fn event_dot_mut_borrow(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} mutably borrows a resource",
        my_name_fmt
    )
}

// 0   1
//     |
// *<--o   the star event (&)
// |   |
pub fn event_dot_static_borrow(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} immutably borrows a resource",
        my_name_fmt
    )
}

// 1   0
//     |
// o<--o
// |   |
// o-->*   the star event (&)
pub fn event_dot_static_reacquire(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is no longer immutably borrowed",
        my_name_fmt
    )
}

// 1   0
//     |
// o<--o
// |
// o-->*   the star event (&mut)
pub fn event_dot_mut_reacquire(my_name: &String, _target_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is no longer mutably borrowed",
        my_name_fmt
    )
}



/* Arrow messages: shows up when someone hovers over an arrow */

// 1   0
//     |
// o<--o
// |
pub fn arrow_move_val_to_val(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is moved to {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// 1   0
//     |
// o<--o
// |   |
pub fn arrow_copy_val_to_val(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is copied to {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// f1  0
//     |
// o<--o
// |
pub fn arrow_move_val_to_func(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is moved to function {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// f1  0
//     |
// o<--o
// |   |
pub fn arrow_copy_val_to_func(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is copied to function {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// 1  f0
//     |
// o<--o
// |
pub fn arrow_move_func_to_val(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "Function {0}'s resource is moved to {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// 1   0
//     |
// o<--o
// |   |
pub fn arrow_static_lend_val_to_val(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is immutably borrowed by {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// 1   0
//     |
// o<->o
//     |
pub fn arrow_static_lend_val_to_func(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is immutably borrowed by function {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// 1   0
//     |
// o<--o
// |
pub fn arrow_mut_lend_val_to_val(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is mutably borrowed by {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// 1   0
//     |
// o<->o
//     |
pub fn arrow_mut_lend_val_to_func(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s resource is mutably borrowed by function {1}",
        from_name_fmt,
        to_name_fmt
    )
}

// 0   1
//     |
// o<--o
// |   |
// o-->o   this arrow (&)
pub fn arrow_static_return(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s immutable borrow of {1}'s resource ends",
        from_name_fmt,
        to_name_fmt
    )
}

// 0   1
//     |
// o<--o
// |
// o-->o   the star event (&mut)
pub fn arrow_mut_return(from_name: &String, to_name: &String) -> String {
    // update styling
    let from_name_fmt = fmt_style(from_name);
    let to_name_fmt = fmt_style(to_name);
    
    format!(
        "{0}'s mutable borrow of {1}'s resource ends",
        from_name_fmt,
        to_name_fmt
    )
}



/* State messages: shows up on the vertical lines for every value/reference */

// The viable is no longer in the scope after this line.
pub fn state_out_of_scope(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} is out of scope",
        my_name_fmt
    )
}

// The resource is transferred on this line or before this line due to move,
// thus it is impossible to access this variable anymore. This is an invisible line in the timeline.
pub fn state_resource_moved(my_name: &String, _to_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource was moved, so {0} no longer has ownership",
        my_name_fmt
    )
}

// temporarily no read or write access right to the resource, but eventually
// the privilege will come back. Occurs when mutably borrowed. This is an invisible line in the timeline.
pub fn state_resource_revoked(my_name: &String, _to_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is mutably borrowed, so it cannot access the resource",
        my_name_fmt,
    )
}

// This ResourceOwner is the unique object that holds the ownership to the underlying resource.
pub fn state_full_privilege(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0} is the owner of the resource", //not necessarily write if let was used rather than let mut
        my_name_fmt
    )
}

/* More than one ResourceOwner has access to the underlying resource
   This means that it is not possible to create a mutable reference
   on the next line.
   About borrow_count: this value is at least one at any time.
        When the first static reference of this ResourceOwner is created,
            this value is set to 1;
        When a new static reference is borrowed from this variable, increment by 1;
        When a static reference goes out of scope, decrement this value by 1;
        When a decrement happens while the borrow_count is 1, the state becomes
            FullPrivilege once again.
*/
pub fn state_partial_privilege(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "{0}'s resource is being shared by one or more variables",
        my_name_fmt
    )
}

// should not appear for visualization in a correct program
pub fn state_invalid(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "something is wrong with the timeline of {0}",
        my_name_fmt
    )
}

pub fn structure(my_name: &String) -> String {
    // update styling
    let my_name_fmt = fmt_style(my_name);
    
    format!(
        "the components in the box belong to struct {0}",
        my_name_fmt
    )
}