module.exports = grammar({
  name: "kanata",

  extras: ($) => [/\s/, $.line_comment, $.block_comment],

  word: ($) => $.identifier,

  rules: {
    source_file: ($) => repeat($.list),

    sexpr: ($) =>
      choice($.number, $.variable, $.alias, $.string, $.identifier, $.list),

    list: ($) => seq("(", $.keyword, repeat($.sexpr), ")"),

    keyword: ($) =>
      choice(
        "defcfg",
        "defsrc",
        "deflayer",
        "deflayermap",
        "defalias",
        "defaliasenvcond",
        "defvar",
        "deftemplate",
        "defseq",
        "defoverrides",
        "deffakekeys",
        "defvirtualkeys",
        "defchords",
        "defzippy",
        "defzippy-experimental",
        "deflocalkeys-win",
        "deflocalkeys-winiov2",
        "deflocalkeys-wintercept",
        "deflocalkeys-linux",
        "deflocalkeys-macos",
      ),

    number: ($) => /-?\d+(?:\.\d+)?/,
    variable: ($) => /\$[A-Za-z_][A-Za-z0-9_-]*/,
    alias: ($) => /@[A-Za-z_][A-Za-z0-9_-]*/,
    identifier: ($) => /[^()\s"]+/,
    string: ($) => /"([^"\\\n]|\\.)*"/,

    line_comment: ($) => token(seq(";;", /.*/)),
    block_comment: ($) => token(seq("#|", /[\s\S]*?/, "|#")),
  },
});
