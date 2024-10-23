if $initialized then [] else
{
define param $initialized value true;

-- keep timestamp of creation for instances as readonly field
define field created ON table instances value time::now() readonly;
define index uid_index ON table instances fields uid UNIQUE;
define index uid_index ON table series fields uid UNIQUE;
define index uid_index ON table studies fields uid UNIQUE;
define field study ON table instances value type::thing("studies",array::slice(record::id(id), 0, 6)) readonly;
define field series ON table instances value type::thing("series",array::slice(record::id(id), 0, 12)) readonly;

DEFINE FUNCTION overwrite fn::register($instance_meta: object, $series_meta: object, $study_meta: object) -> any {
    return IF record::exists($instance_meta.id) {
    	(SELECT * FROM ONLY $instance_meta)
    } ELSE {
        IF !record::exists($series_meta.id) {
            IF !record::exists($study_meta.id) {
                insert into studies $study_meta;
            };
            INSERT INTO series $series_meta;
        };
        INSERT INTO instances $instance_meta;
        (true)
    }
};

-- set up aggregates for fast lookup of overall size an count of instances per series/study
define table instances_per_studies as select count(), math::sum(file.size) as size, study from instances group by study;
define table instances_per_series as select count(), math::sum(file.size) as size, series from instances group by series;
}
end
