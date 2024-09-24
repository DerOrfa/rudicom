if $initialized then [] else
{
define param $initialized value true;

-- set up one-day change log for instances
define table instances changefeed 1d;
-- keep timestamp of creation for instances as readonly field
define field created ON table instances value time::now() readonly;

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

-- when an instance is added, add it to the instances list of its parent series
-- also update last_update on the series
define event add_instance on table instances when $event = "CREATE"	then
(
    update $after.series set instances += $after.id, last_update = time::now() return none
);
-- when a series is added, add it to the series list of its parent study
define event add_series on table series when $event = "CREATE" then
(
    update $after.study set series += $after.id return none
);

-- when an instance is removed, remove it from the instances list of its parent series and update last_update on the series
-- remove the whole series if it would be empty after that
define event del_instance on table instances when $event = "DELETE" then
(
    if array::len($before.series.instances)>1
    then
        update $before.series set instances -= $before.id, last_update = time::now() return none
    else
        delete $before.series
    end
);
-- when a series is removed, remove it from the series list of its parent study
-- remove the whole study if it would be empty after that
define event del_series on table series when $event = "DELETE" then
(
    if array::len($before.study.series)>1
    then
        update $before.study set series -= $before.id return none
    else
        delete $before.study
    end
);

-- set up aggregates for fast lookup of overall size an count of instances per series/study
define table instances_per_studies as select count(), math::sum(file.size) as size, series.study as me from instances group by series.study;
define table instances_per_series as select count(), math::sum(file.size) as size, series as me from instances group by series;
}
end
