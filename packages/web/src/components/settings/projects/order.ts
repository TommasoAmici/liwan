/** Move the item at `index` one place up (-1) or down (1), returning a new array. */
export const moveProject = <T>(projects: T[], index: number, direction: -1 | 1): T[] => {
	const target = index + direction;
	if (index < 0 || index >= projects.length || target < 0 || target >= projects.length) return projects;

	const next = [...projects];
	[next[index], next[target]] = [next[target], next[index]];
	return next;
};
