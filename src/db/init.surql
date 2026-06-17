// set up instances table
DEFINE FIELD IF NOT EXISTS timestamp ON instances TYPE datetime DEFAULT time::now() READONLY;
DEFINE FIELD IF NOT EXISTS series ON instances TYPE record<series> REFERENCE;
DEFINE EVENT OVERWRITE del_instance ON TABLE instances WHEN $event == "DELETE" ASYNC THEN
{
    IF $value.series.instances.is_array() AND $value.series.instances.is_empty() {delete $value.series}
};

// set up series table
DEFINE FIELD IF NOT EXISTS timestamp ON series TYPE datetime DEFAULT time::now() READONLY;
DEFINE FIELD IF NOT EXISTS study ON series TYPE record<studies> REFERENCE;
DEFINE FIELD IF NOT EXISTS instances ON series COMPUTED <~ instances;
DEFINE EVENT OVERWRITE del_series ON TABLE series WHEN $event == "DELETE" ASYNC THEN
{
    IF $value.study.series.is_array() AND $value.study.series.is_empty() {delete $value.study}
};

// set up studies table
DEFINE FIELD IF NOT EXISTS timestamp ON studies TYPE datetime DEFAULT time::now() READONLY;
DEFINE FIELD IF NOT EXISTS series ON studies COMPUTED <~ series;
