if exists("b:current_syntax")
  finish
endif

" Layer Kotlin on top of BMake: Kotlin control lines (val/var/fun/if/for/
" while/class/object/import/package/braces) get real Kotlin highlighting,
" everything else stays BMake DSL — mirrors the CLI's own .bm.kts transpiler.
syntax include @bmakeKotlin syntax/kotlin.vim
unlet! b:current_syntax

runtime syntax/bmake.vim
unlet! b:current_syntax

syntax region bmakeKotlinLine start="^\s*\(val\|var\|fun\|class\|object\|if\|else\|for\|while\|import\s\+\S\|package\)\>" end="$" contains=@bmakeKotlin keepend

let b:current_syntax = "bmake_kts"