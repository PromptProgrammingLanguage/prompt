" Vim syntax file
" Language: Prompt
" Maintainer: Jesse Tuchsen

set filetype=prompt

" Create contained Bash syntax
syntax include @Bash syntax/bash.vim
unlet b:current_syntax
syntax region bashCodeContained start='`' end='`' contained contains=@Bash

" Create contained YAML syntax
syntax include @Yaml syntax/yaml.vim
unlet b:current_syntax
syntax region yamlCodeContained start=+^\s\+direction\ze: + end=+^\({\)\@=+ contained contains=@Yaml

syntax region yamlCode start=+^\s\+\%(description\|direction\|eager\|history\)\ze: + end=+^\({\)\@=+ contains=@Yaml,yamlCodeContained,bashCodeContained
syntax region bashCode start='`' end='`' keepend contains=@Bash,bashCodeContained,yamlCodeContained

syntax match promptName '^[A-Z|a-z|0-9|_]\+$' nextgroup=yamlCode
syntax match promptCall '[A-Z|a-z|0-9|_]\+,'
syntax keyword promptMatch match nextgroup=promptMatchVariable skipwhite
syntax match promptMatchVariable '$[A-Z]\+'

syntax match promptRegex '([^)]*)'
syntax match promptFatArrow '=>'
syntax match promptSkinnyArrow '->'

hi def link promptName Type
hi def link promptCall Type
hi def link promptMatch Statement
hi def link promptMatchVariable PreProc
hi def link promptFatArrow Statement
hi def link promptSkinnyArrow Statement
hi def link promptRegex Comment
