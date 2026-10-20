# Literal path fixtures shared by module/parser evaluation and VM activation.
{
  default = "/run/underwrap-runtime";
  custom = "/run/underwrap-test-runtime";
  root = "/";
  percent = "/run/underwrap-literal/%t/%u/%%/%";
  backslash = "/run/underwrap-literal/back\\x20slash/line\\n/tab\\t/end\\";
  double-quote = "/run/underwrap-literal/quo\"te";
  single-quote = "/run/underwrap-literal/quo'te";
  unicode = "/run/underwrap-literal/café-雪";
  punctuation = "/run/underwrap-literal/#:$;*?[a]{b}~=@+!";
  combined = "/run/underwrap-literal/%t/back\\x20slash/quo\"te's";
}
