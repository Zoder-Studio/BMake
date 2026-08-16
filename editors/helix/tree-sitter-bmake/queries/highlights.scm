(comment) @comment

"Version" @keyword
"Start" @keyword.control
"Stop" @keyword.control

(task "Task" @tag name: (identifier) @function)
"</Task>" @tag
"<" @punctuation.bracket
">" @punctuation.bracket

(import_statement "import" @keyword.control.import)
(import_statement path: (value) @string.special.path)

(directive name: (directive_name) @property)
(directive value: (value) @string)

"Command" @keyword.other
"{" @punctuation.bracket
"}" @punctuation.bracket

((value) @constant.builtin.boolean
  (#match? @constant.builtin.boolean "^(true|false)$"))

((value) @constant.numeric
  (#match? @constant.numeric "^[0-9]+(\\.[0-9]+)*$"))

":" @punctuation.delimiter
"=" @operator