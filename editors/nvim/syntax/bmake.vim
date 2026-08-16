if exists("b:current_syntax")
  finish
endif

syntax case match

syntax match bmakeComment "//.*$"
syntax region bmakeBlockComment start="/\\+" end="+\\\\"

syntax match bmakeVersionTag "<Version:\s*[^>]\+>"
syntax match bmakeSection "^\s*\(Start\|Stop\)\s*$"

syntax match bmakeTaskTag "<Task:\s*[^>]\+>"
syntax match bmakeTaskTag "</Task>"

syntax match bmakeImport "^\s*import\s*="

syntax match bmakeDirective "^\s*\(Lang\|System\|Sub-System\|Platform\|Arch\|Shell\|Runs-on\|version\|Remote\|Env\|Environment\|Dependency\|Need\|Tool\|Artifact\|Condition\|Clean\|Workdir\|Directory\|Input\|Output\|Depends-on\|Timeout\|StopOnError\|Cache\|Parallel\|Profile\|Log-level\|Require\|Command\|Before\|After\|Rename\|OnError\|Retry\|Source\):"

syntax keyword bmakeBoolean true false
syntax match bmakeNumber "\<\d\+\(\.\d\+\)*\>"
syntax match bmakeContinuation "+/\s*$"
syntax match bmakeEnvVar "\<[A-Za-z_][A-Za-z0-9_]*\>="me=e-1
syntax match bmakeOperator "==\|->\|="

highlight default link bmakeComment Comment
highlight default link bmakeBlockComment Comment
highlight default link bmakeVersionTag Keyword
highlight default link bmakeSection Statement
highlight default link bmakeTaskTag Tag
highlight default link bmakeImport Include
highlight default link bmakeDirective Identifier
highlight default link bmakeBoolean Boolean
highlight default link bmakeNumber Number
highlight default link bmakeContinuation Operator
highlight default link bmakeEnvVar PreProc
highlight default link bmakeOperator Operator

let b:current_syntax = "bmake"