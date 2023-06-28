## RustvizParse -- src -- parser.rs

**Parse.rs** parse through the AST tree provided by syn library and in turn make the header for future steps(information about Owners and functions). The purpose of parse.rs seems to be to fill the preprocess_external_events member of the visualization data struct (what is needed for svg rendering)

### 1.Hearders

**syn** is a parsing library for parsing a stream of Rust tokens into a syntax tree of Rust source code.
**log** provides macros to log at various levels, capture information about the running program.


### 2.Functions

**`path_fmt`**: This funtion takes an ExprPath and formats it to a string representation of the path.

**`parse`**:
- This function first opens the file specified by the path, reads its contents, and parses it using **`syn::parse_file`**.
- The resulting AST (Abstract Syntax Tree) is passed to the **`str_gen`** function, and the generated header string is returned.

**`str_gen`**:
- This function takes a **`syn::File`** (AST) and generates a header string based on the variable definitions found in the AST.
- It iterates over the items in the AST and calls the **`get_info`** function to collect variable definitions.
- It constructs the header string based on the collected variable information.

**`get_info`**:
- This function takes an AST and a mutable empty **`HashSet`** (**`var_def`**) to store variable definitions.
- It iterates over the items in the AST and handles function definitions (**`Item::Fn`**) by inserting them into the **`var_def`** set.
- For function arguments and statements, it calls the **`parse_stmt`** function to extract variable information and inserts it into **`var_def`**.

**`parse_expr`**:
- This function takes an **`Expr`** and parses it to extract variable information and updates the **`local`** variable accordingly.
- Depending on the type of expression (**`Expr::Call`**, **`Expr::MethodCall`**, **`Expr::Reference`**, **`Expr::Block`**, etc.), it handles different scenarios and updates **`local`** and **`var_def`**.

**`parse_stmt`**:
- This function takes a **`Stmt`** (statement) and parses it to extract variable information and updates **`var_def`**.
- It handles different statement types (**`Stmt::Local`**, **`Stmt::Semi`**, **`Stmt::Expr`**, etc.) and calls **`parse_expr`** to handle the expressions within the statement.

## RustvizParse -- src -- main.rs

- It uses the clap crate to define and handle command-line arguments. In this case, it defines a single command-line argument "target" which specifies the target file to parse.
- The script first gets the target filename from the command line arguments, then uses the parse function to parse the file, then reads the original contents of the file. It then creates a new file named **"main.rs"** in the same directory as the original file and writes the parse results and original contents to this new file.