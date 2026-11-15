import type { FunnelStep } from "@/constants";

export const matchLabels = {
	equal: "is exactly",
	contains: "contains",
	starts_with: "starts with",
} as const;

export type FunnelMatchType = keyof typeof matchLabels;

export const matchTypes = Object.keys(matchLabels) as FunnelMatchType[];

/** A funnel step while it is being edited, with a concrete match type. */
export type EditorStep = { type: "page"; path: string; matchType: FunnelMatchType } | { type: "event"; name: string };

export const editorStepFrom = (step: FunnelStep): EditorStep =>
	step.type === "event"
		? { type: "event", name: step.name }
		: { type: "page", path: step.path, matchType: (step.matchType ?? "equal") as FunnelMatchType };

export const funnelStepFrom = (step: EditorStep): FunnelStep =>
	step.type === "event"
		? ({ type: "event", name: step.name.trim() } as FunnelStep)
		: ({ type: "page", path: step.path.trim(), matchType: step.matchType } as FunnelStep);
