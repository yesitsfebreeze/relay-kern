; Minimal JS highlight query. Reused by TS via injection of additional terms.

[
  "as" "async" "await" "break" "case" "catch" "class" "const" "continue"
  "debugger" "default" "delete" "do" "else" "export" "extends" "finally"
  "for" "from" "function" "get" "if" "import" "in" "instanceof" "let"
  "new" "of" "return" "set" "static" "switch" "throw" "try" "typeof"
  "var" "void" "while" "with" "yield"
] @keyword

(true) @constant
(false) @constant
(null) @constant
(undefined) @constant

(number) @number

(string) @string
(template_string) @string
(regex) @string

(comment) @comment

(function_declaration name: (identifier) @function)
(method_definition name: (property_identifier) @function)
(call_expression function: (identifier) @function)
(call_expression function: (member_expression property: (property_identifier) @function))

(class_declaration name: (identifier) @type)
(new_expression constructor: (identifier) @type)

[
  "+" "-" "*" "/" "%" "**" "=" "==" "===" "!=" "!==" "<" ">" "<=" ">="
  "&&" "||" "!" "??" "=>" "?" "..."
] @operator

[ "(" ")" "[" "]" "{" "}" "," ";" ":" "." ] @punctuation

(identifier) @variable
