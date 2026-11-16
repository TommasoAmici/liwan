import { describe, expect, it } from "bun:test";

import { moveProject } from "./order";

describe("moveProject", () => {
	it("moves the first project down and leaves it first when moved up", () => {
		expect(moveProject(["a", "b", "c"], 0, 1)).toEqual(["b", "a", "c"]);
		expect(moveProject(["a", "b", "c"], 0, -1)).toEqual(["a", "b", "c"]);
	});

	it("swaps a middle project with its neighbour in either direction", () => {
		expect(moveProject(["a", "b", "c"], 1, -1)).toEqual(["b", "a", "c"]);
		expect(moveProject(["a", "b", "c"], 1, 1)).toEqual(["a", "c", "b"]);
	});

	it("moves the last project up and leaves it last when moved down", () => {
		expect(moveProject(["a", "b", "c"], 2, -1)).toEqual(["a", "c", "b"]);
		expect(moveProject(["a", "b", "c"], 2, 1)).toEqual(["a", "b", "c"]);
	});

	it("leaves the order alone for an index outside the list", () => {
		expect(moveProject(["a", "b"], -1, 1)).toEqual(["a", "b"]);
		expect(moveProject(["a", "b"], 2, -1)).toEqual(["a", "b"]);
		expect(moveProject([], 0, 1)).toEqual([]);
	});
});
