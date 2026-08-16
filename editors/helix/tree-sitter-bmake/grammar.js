module.exports = grammar({
  name: 'bmake',

  extras: $ => [/\s/, $.comment],

  rules: {
    source_file: $ => repeat(choice(
      $.version_tag,
      $.section_keyword,
      $.task,
      $.import_statement,
      $.directive,
    )),

    comment: $ => choice(
      seq('//', /.*/),
      seq('/+', /[^]*?/, '+\\'),
    ),

    version_tag: $ => seq('<', 'Version', ':', field('value', $.value), '>'),

    section_keyword: $ => choice('Start', 'Stop'),

    task: $ => seq(
      '<', 'Task', ':', field('name', $.identifier), '>',
      repeat($.task_field),
      '</', 'Task', '>',
    ),

    task_field: $ => choice(
      $.directive,
      $.command_block,
    ),

    import_statement: $ => seq('import', '=', field('path', $.value)),

    directive: $ => seq(
      field('name', $.directive_name),
      ':',
      field('value', optional($.value)),
    ),

    directive_name: $ => /[A-Za-z][A-Za-z0-9_-]*/,

    command_block: $ => seq(
      'Command', ':', '{',
      repeat($.command_line),
      '}',
    ),

    command_line: $ => /[^\n}]+/,

    value: $ => /[^\n]+/,

    identifier: $ => /[^\n>]+/,
  }
});