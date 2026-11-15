import styles from "./funnels.module.css";

import type { FormEvent } from "react";
import { useId, useState } from "react";
import { Autocomplete } from "@base-ui/react/autocomplete";
import { ArrowDownIcon, ArrowUpIcon, XIcon } from "lucide-react";

import { api, queryClient, useMutation } from "@/api";
import type { DateRange } from "@/api/ranges";
import { Dialog } from "@/components/ui/dialog";
import { createToast } from "@/components/ui/toast";
import type { Funnel, ProjectResponse } from "@/constants";
import { invalidateFunnels, useDimension } from "@/hooks/api";
import { formatMetricVal } from "@/utils";
import { type EditorStep, editorStepFrom, type FunnelMatchType, funnelStepFrom, matchLabels, matchTypes } from "./step";

type PathSuggestion = { path: string; views: number };

const emptySteps = (): EditorStep[] => [
	{ type: "page", path: "", matchType: "equal" },
	{ type: "page", path: "", matchType: "equal" },
];

const errorMessage = (error: unknown, fallback: string) => {
	const message = (error as { message?: unknown } | null)?.message;
	return typeof message === "string" && message ? message : fallback;
};

export const FunnelDialog = ({
	projectId,
	project,
	range,
	funnel,
	customEventsHidden,
	open,
	onOpenChange,
	onSaved,
}: {
	projectId: string;
	project: ProjectResponse;
	range: DateRange;
	funnel?: Funnel | null;
	customEventsHidden: boolean;
	open: boolean;
	onOpenChange: (open: boolean) => void;
	onSaved: (id?: string) => void;
}) => {
	const [displayName, setDisplayName] = useState(funnel?.displayName ?? "");
	const [steps, setSteps] = useState<EditorStep[]>(() => (funnel ? funnel.steps.map(editorStepFrom) : emptySteps()));
	const formId = useId();
	const hiddenEventStep = customEventsHidden && steps.some((step) => step.type === "event");
	// Suggestions follow the selected range, most-viewed first. Any path can still
	// be typed, so a page that has no traffic yet is not blocked.
	const pathRows = useDimension({ project, dimension: "path", metric: "views", range, filters: [] });
	const pathSuggestions = (pathRows.data ?? [])
		.flatMap((row) => (row.dimensionValue && row.value ? [{ path: row.dimensionValue, views: row.value }] : []))
		.slice(0, 100);

	const { mutate, isPending } = useMutation({
		mutationFn: async (body: { displayName: string; steps: ReturnType<typeof funnelStepFrom>[] }) => {
			if (funnel) {
				await api["/api/dashboard/project/{project_id}/funnels/{funnel_id}"].put({
					params: { project_id: projectId, funnel_id: funnel.id },
					json: body,
				});
				return undefined;
			}
			const created = await api["/api/dashboard/project/{project_id}/funnels"]
				.post({ params: { project_id: projectId }, json: body })
				.json();
			return created as Funnel;
		},
		onSuccess: async (created) => {
			await invalidateFunnels(projectId);
			createToast(funnel ? "Funnel updated" : "Funnel created", "success");
			onSaved(funnel?.id ?? created?.id);
			onOpenChange(false);
		},
		onError: (error) =>
			createToast(errorMessage(error, funnel ? "Failed to update funnel" : "Failed to create funnel"), "error"),
	});

	const submit = (event: FormEvent<HTMLFormElement>) => {
		event.preventDefault();
		if (isPending || hiddenEventStep) return;
		mutate({ displayName: displayName.trim(), steps: steps.map(funnelStepFrom) });
	};

	const update = (index: number, step: EditorStep) =>
		setSteps((current) => current.map((item, position) => (position === index ? step : item)));

	const move = (index: number, offset: -1 | 1) =>
		setSteps((current) => {
			const next = [...current];
			const target = index + offset;
			if (target < 0 || target >= next.length) return current;
			[next[index], next[target]] = [next[target], next[index]];
			return next;
		});

	const remove = (index: number) => setSteps((current) => current.filter((_, position) => position !== index));

	return (
		<Dialog
			open={open}
			onOpenChange={onOpenChange}
			title={funnel ? "Edit funnel" : "New funnel"}
			className={styles.editorDialog}
			autoOverflow
		>
			<form onSubmit={submit} className={styles.form}>
				<label>
					Name
					<input
						type="text"
						value={displayName}
						onChange={(event) => setDisplayName(event.currentTarget.value)}
						required
						maxLength={100}
						autoFocus
					/>
				</label>
				{hiddenEventStep && <p className={styles.warning}>Custom events are hidden for this project.</p>}
				<ol className={styles.steps}>
					{steps.map((step, index) => (
						<li key={index} className={styles.stepRow}>
							<span className={styles.stepLabel}>Step {index + 1}</span>
							<select
								aria-label={`Step ${index + 1} type`}
								value={step.type}
								onChange={(event) =>
									update(
										index,
										event.currentTarget.value === "event"
											? { type: "event", name: "" }
											: { type: "page", path: "", matchType: "equal" },
									)
								}
							>
								<option value="page">Page</option>
								<option value="event" disabled={customEventsHidden}>
									Custom event
								</option>
							</select>
							<div className={styles.stepFields}>
								{step.type === "page" ? (
									<>
										<label className={styles.stepField} htmlFor={`${formId}-path-${index}`}>
											Path
											<Autocomplete.Root
												items={pathSuggestions}
												value={step.path}
												onValueChange={(value) => update(index, { ...step, path: value })}
												itemToStringValue={(item: PathSuggestion) => item.path}
												filter={(item: PathSuggestion, query: string) =>
													item.path.toLowerCase().includes(query.toLowerCase())
												}
												openOnInputClick
												autoHighlight="always"
											>
												<Autocomplete.Input
													id={`${formId}-path-${index}`}
													type="text"
													required
													maxLength={2048}
													placeholder="/pricing"
													aria-label={`Step ${index + 1} path`}
												/>
												<Autocomplete.Portal>
													<Autocomplete.Positioner className={styles.pathPositioner} sideOffset={4}>
														<Autocomplete.Popup className={styles.pathPopup}>
															<Autocomplete.List className={styles.pathList}>
																{(item: PathSuggestion) => (
																	<Autocomplete.Item key={item.path} value={item} className={styles.pathItem}>
																		<span className={styles.pathName}>{item.path}</span>
																		<span className={styles.pathCount}>
																			{formatMetricVal(item.views, "views")} views
																		</span>
																	</Autocomplete.Item>
																)}
															</Autocomplete.List>
															<Autocomplete.Empty className={styles.pathEmpty}>
																No paths in this range.
															</Autocomplete.Empty>
														</Autocomplete.Popup>
													</Autocomplete.Positioner>
												</Autocomplete.Portal>
											</Autocomplete.Root>
										</label>
										<label className={styles.stepField}>
											Match
											<select
												value={step.matchType}
												onChange={(event) =>
													update(index, {
														...step,
														matchType: event.currentTarget.value as FunnelMatchType,
													})
												}
											>
												{matchTypes.map((type) => (
													<option key={type} value={type}>
														{matchLabels[type]}
													</option>
												))}
											</select>
										</label>
										<p className={styles.hint}>Use the path after the domain, without a trailing slash.</p>
									</>
								) : (
									<>
										<label className={styles.stepField}>
											Event name
											<input
												type="text"
												value={step.name}
												onChange={(event) => update(index, { ...step, name: event.currentTarget.value })}
												required
												maxLength={255}
												placeholder="signup"
											/>
										</label>
										{customEventsHidden && (
											<p className={styles.warning}>
												Custom events are hidden for this project. Convert this step to a page or remove it before
												saving.
											</p>
										)}
									</>
								)}
							</div>
							<div className={styles.stepActions}>
								<button
									type="button"
									className="button-ghost"
									aria-label={`Move step ${index + 1} up`}
									onClick={() => move(index, -1)}
									disabled={index === 0}
								>
									<ArrowUpIcon size={16} />
								</button>
								<button
									type="button"
									className="button-ghost"
									aria-label={`Move step ${index + 1} down`}
									onClick={() => move(index, 1)}
									disabled={index === steps.length - 1}
								>
									<ArrowDownIcon size={16} />
								</button>
								<button
									type="button"
									className="button-ghost"
									aria-label={`Remove step ${index + 1}`}
									onClick={() => remove(index)}
									disabled={steps.length <= 2}
								>
									<XIcon size={16} />
								</button>
							</div>
						</li>
					))}
				</ol>
				<div>
					<button
						type="button"
						className="button-secondary"
						onClick={() => setSteps((current) => [...current, { type: "page", path: "", matchType: "equal" }])}
						disabled={steps.length >= 10}
					>
						Add step
					</button>
				</div>
				<div className="action-row">
					<Dialog.Close className="button-secondary">Cancel</Dialog.Close>
					<button type="submit" disabled={isPending || hiddenEventStep}>
						{funnel ? "Save changes" : "Create funnel"}
					</button>
				</div>
			</form>
		</Dialog>
	);
};

export const DeleteFunnelDialog = ({
	projectId,
	funnel,
	open,
	onOpenChange,
	onDeleted,
}: {
	projectId: string;
	funnel: Funnel;
	open: boolean;
	onOpenChange: (open: boolean) => void;
	onDeleted: () => void;
}) => {
	const { mutate, isPending } = useMutation({
		mutationFn: () =>
			api["/api/dashboard/project/{project_id}/funnels/{funnel_id}"].delete({
				params: { project_id: projectId, funnel_id: funnel.id },
			}),
		onSuccess: async () => {
			// Refresh the list only: invalidating the report would refetch it for a
			// funnel that no longer exists before the selection moves.
			onDeleted();
			onOpenChange(false);
			createToast("Funnel deleted", "success");
			await queryClient.invalidateQueries({ queryKey: ["funnels", projectId] });
		},
		onError: (error) => createToast(errorMessage(error, "Failed to delete funnel"), "error"),
	});

	return (
		<Dialog
			open={open}
			onOpenChange={onOpenChange}
			title={`Delete funnel: ${funnel.displayName}`}
			description="This removes the saved funnel. Event data is not deleted."
		>
			<div className="action-row">
				<Dialog.Close className="button-secondary">Cancel</Dialog.Close>
				<button type="button" className="button-danger" onClick={() => mutate()} disabled={isPending}>
					Delete funnel
				</button>
			</div>
		</Dialog>
	);
};
