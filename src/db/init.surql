DEFINE FIELD IF NOT EXISTS timestamp ON instances TYPE datetime DEFAULT time::now();
DEFINE FIELD IF NOT EXISTS series ON instances TYPE record<series> REFERENCE;

DEFINE FIELD IF NOT EXISTS timestamp ON series TYPE datetime DEFAULT time::now();
DEFINE FIELD IF NOT EXISTS study ON series TYPE record<studies> REFERENCE;
DEFINE FIELD IF NOT EXISTS instances ON series COMPUTED <~ instances;

DEFINE FIELD IF NOT EXISTS timestamp ON studies TYPE datetime DEFAULT time::now();
DEFINE FIELD IF NOT EXISTS series ON studies COMPUTED <~ series;
