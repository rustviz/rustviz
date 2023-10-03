pub enum LineStyle {
    OwnerLine,
    RefValueLine,
    RefDataLine,
}

/* - `OwnerLine`: Indicates the line style for owner resources. It is used to represent the current status of the owner. It has four variants:
   - `Solid`: Represents that the owner can be assigned to, and read from and written to.
   - `Hollow`: Represents that the owner can only be read from but not written to.
   - `Dotted`: Represents that the owner cannot be read from or written to temporarily because it is borrowed by a mutable reference.
   - `Empty`: Represents that the owner cannot be read from or written to anymore because it has been moved.
*/
pub enum OwnerLine {
    // what is the owner's current status?
    Solid,     // you can assign to, write and read from this owner
    Hollow,    // you can only read the data from this RAP.
    Dotted,    // you cannot read nor write the data from this RAP temporarily (borrowed away by a mut reference)
    Empty,     // you cannot read nor write the data from this RAP forever (moved)
}


/* - `Reassignable`: Represents that the reference is declared as a mutable reference (**`&mut`**) and currently no second-level reference is borrowing from it, so it can be reassigned.
   - `NotReassignable`: Represents that the reference is not reassignable for any other cases.
*/
pub enum RefValueLine {
    // can you reassign the ref (self) to something else right after this line? Depend on & or &mut 
    // (i.e. static object will never be re-assignable. For mutable objects, it depends)
    // if there is a current borrowing on a mutable object, then it is not reassignable
    
    // This will show up as a colorful line
    Reassignable,           // this ref is declared as 
                            // mut ref = &x;
                            // or 
                            // mut ref = &mut x;
                            // and currently no second-level reference is borrowing from it. So you can in fact reassign it. 
    // This will be a grayed out line
    NotReassignable,        // anything else.
}

/* - `Solid`: Represents that the data can be read from and written to.
   - `Hollow`: Represents that the data can only be read.
   - `Dotted`: Represents that the data cannot be read from or written to because the reference is borrowed by another mutable reference.
   - `Empty`: Represents that the data cannot be read from or written to anymore because it has been moved or is no longer in use.
*/
pub enum RefDataLine {
    // can you change the data this reference points to? I.e. you might need to dereference multiple times
    Solid,     // we can r/w data
    Hollow,    // can only read data
    Dotted,    // cannot read or write, because self is borrowed by yet another &mut
    Empty,     // you cannot read nor write the data from this RAP forever (after last use)
}
