; Minimal Rust highlight query for relay TUI.
; Captures map 1:1 to HighlightRole names.

; keywords
[
  "as" "async" "await" "break" "const" "continue" "default" "dyn" "else"
  "enum" "extern" "fn" "for" "if" "impl" "in" "let" "loop" "match" "mod"
  "move" "pub" "ref" "return" "static" "struct" "trait" "type"
  "union" "unsafe" "use" "where" "while"
] @keyword

(mutable_specifier) @keyword

; types
(type_identifier) @type
(primitive_type) @type

; functions
(function_item name: (identifier) @function)
(call_expression function: (identifier) @function)
(call_expression function: (field_expression field: (field_identifier) @function))
(call_expression function: (scoped_identifier name: (identifier) @function))
(macro_invocation macro: (identifier) @function)

; strings
(string_literal) @string
(raw_string_literal) @string
(char_literal) @string

; numbers
(integer_literal) @number
(float_literal) @number
(boolean_literal) @constant

; comments
(line_comment) @comment
(block_comment) @comment

; attributes
(attribute_item) @attribute
(inner_attribute_item) @attribute

; operators / punctuation
[
  "+" "-" "*" "/" "%" "=" "==" "!=" "<" ">" "<=" ">=" "&&" "||" "!"
  "&" "|" "^" "<<" ">>" "+=" "-=" "*=" "/=" "%=" "&=" "|=" "^="
  "<<=" ">>=" "->" "=>" ".." "..=" "?"
] @operator

[ "(" ")" "[" "]" "{" "}" "," ";" ":" "::" "." ] @punctuation

; identifiers (lowest priority, last)
(identifier) @variable
