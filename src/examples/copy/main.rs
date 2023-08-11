/* --- BEGIN Variable Definitions ---
Owner x; Owner y
--- END Variable Definitions --- */
fn main() {
    let x = 5; // !{ Bind(x) }
    let y = x; // !{ Copy(x->y) }
    /* !{ Lifetime@Func(max:'a)(x[3:5];y[4:6]->r[3;6]) } */
    k = max(x,y); /* !{ Lifetime@Part('i1)(request_queue[1:23];read_request[4:6];
   &read_request[6:20]; update_request[8:20];
   &update_request[9:20]; delete_request[12:20];
   &delete_request[13:20]; &request_queue[18:18]
 )}
 */
} /* !{
    GoOutOfScope(x),
    GoOutOfScope(y)
} */