if $initialized then [] else
{
define param $initialized value true;

-- set up one-day change log for instances
define table instances changefeed 1d;
-- keep timestamp of creation for instances as readonly field
define field created ON table instances value time::now() readonly;

-- when an instance is added, add it to the instances list of its parent series
-- also update last_update on the series
define event add_instance on table instances when $event = "CREATE"	then
(
    relate ($after.series)->parent->($after.id) return none
);
-- when a series is added, add it to the series list of its parent study
define event add_series on table series when $event = "CREATE" then
(
    relate ($after.study)->parent->($after.id) return none
);

-- when an instance is removed, remove it from the instances list of its parent series and update last_update on the series
-- remove the whole series if it would be empty after that
define event del_instance on table instances when $event = "DELETE" then
(
	if (select count from only instances_per_series:[$before.series]).count<1
	then
        delete $before.series
	end
);
-- when a series is removed, remove it from the series list of its parent study
-- remove the whole study if it would be empty after that
define event del_series on table series when $event = "DELETE" then
(
	if (select count from only instances_per_studies:[$before.study]).count<1
	then
        delete $before.study
	end
);

-- set up aggregates for fast lookup of overall size an count of instances per series/study
define table instances_per_studies as select count(), math::sum(file.size) as size, series.study as me from instances group by series.study;
define table instances_per_series as select count(), math::sum(file.size) as size, series as me from instances group by series;
}
end
