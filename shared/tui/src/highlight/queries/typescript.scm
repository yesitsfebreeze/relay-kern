; Minimal TypeScript highlight query.

[
  "as" "async" "await" "break" "case" "catch" "class" "const" "continue"
  "debugger" "default" "delete" "do" "else" "export" "extends" "finally"
  "for" "from" "function" "get" "if" "implements" "import" "in" "instanceof"
  "interface" "let" "new" "of" "private" "protected" "public" "readonly"
  "return" "set" "static" "switch" "throw" "try" "type" "typeof" "var"
  "void" "while" "with" "yield" "namespace" "abstract" "declare" "enum"
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

(predefined_type) @type
(type_identifier) @type

(function_declaration name: (identifier) @function)
(method_definition name: (property_identifier) @function)
(call_expression function: (identifier) @function)
(call_expression function: (member_expression property: (property_identifier) @function))

(class_declaration name: (type_identifier) @type)
(interface_declaration name: (type_identifier) @type)

[
  "+" "-" "*" "/" "%" "**" "=" "==" "===" "!=" "!==" "<" ">" "<=" ">="
  "&&" "||" "!" "??" "=>" "?" "..."
] @operator

[ "(" ")" "[" "]" "{" "}" "," ";" ":" "." ] @punctuation

(identifier) @variable
