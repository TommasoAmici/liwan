import styles from "./funnels.module.css";

import { useState } from "react";
import { FileTextIcon, ZapIcon } from "lucide-react";

import { LoadingSpinner } from "@/components/ui/loading";
import { getDashboardProjectId } from "@/config";
import type { DimensionFilter, Funnel, FunnelStep } from "@/constants";
import { dimensions } from "@/constants";
import { useFunnelReport, useFunnels, useMe, useProject } from "@/hooks/api";
import { useRange } from "@/hooks/persist";
import { getUsername } from "@/utils";
import { SelectFilters } from "../filter";
import { formatMetricVal, formatPercent } from "../metric-format";
import { ProjectHeader } from "../project-header";
import { ProjectNav } from "../project-nav";
import { SelectRange } from "../range";
import { DeleteFunnelDialog, FunnelDialog } from "./dialog";
import { matchLabels } from "./step";

const adminEmpty = "No funnels yet. Create a funnel to see how visitors move from one step to the next.";
const viewerEmpty = "No funnels have been set up for this project.";

const stepName = (step: FunnelStep) => (step.type === "page" ? step.path : step.name);

const StepIcon = ({ step }: { step: FunnelStep }) =>
	step.type === "page" ? <FileTextIcon size={15} /> : <ZapIcon size={15} />;

export const Funnels = () => {
	const [projectId] = useState(() => (typeof window === "undefined" ? undefined : getDashboardProjectId()));
	const { project, notFound } = useProject(projectId);
	// Anonymous visitors of a public project must not ask for a session they do not have.
	const { role } = useMe({ enabled: Boolean(getUsername()) });
	const isAdmin = role === "admin";
	const { range, setRange } = useRange();
	const { funnels, isLoading: funnelsLoading, error: funnelsError } = useFunnels(project?.id);
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const [filters, setFilters] = useState<DimensionFilter[]>([]);
	const [editor, setEditor] = useState<{ funnel: Funnel | null } | null>(null);
	const [deleting, setDeleting] = useState<Funnel | null>(null);

	const selected = funnels.find((funnel) => funnel.id === selectedId) ?? funnels[0];
	const report = useFunnelReport({ projectId: project?.id, funnelId: selected?.id, range, filters });
	const customEventsHidden = project?.customEventsDisplay === "hide";

	if (!projectId || notFound) {
		return <div className={styles.notFound}>Project not found</div>;
	}

	if (!project) {
		return (
			<div className={styles.loading} role="status" aria-label="Loading project">
				<LoadingSpinner />
			</div>
		);
	}

	const filterDimensions = [...dimensions, "property" as const].filter(
		(dimension) =>
			!project.hiddenDimensions.includes(dimension) && dimension !== "url_entry" && dimension !== "url_exit",
	);
	const counts = report.report?.counts ?? [];
	const first = counts[0] ?? 0;
	const last = counts[counts.length - 1] ?? 0;
	const completion = first > 0 ? (last / first) * 100 : 0;
	const countsHidden = report.hiddenEvents || report.error !== null;
	const createButton = isAdmin && (
		<button type="button" onClick={() => setEditor({ funnel: null })}>
			New funnel
		</button>
	);

	return (
		<div className={styles.screen}>
			<div className={styles.header}>
				<ProjectHeader project={project} />
				<SelectRange onSelect={setRange} range={range} projectId={project.id} />
			</div>
			<ProjectNav projectId={project.id} current="funnels" />
			{funnelsLoading ? (
				<div className={styles.loading} role="status" aria-label="Loading funnels">
					<LoadingSpinner />
				</div>
			) : funnelsError ? (
				<article className={styles.card}>
					<p className={styles.message}>Failed to load funnels.</p>
				</article>
			) : funnels.length === 0 ? (
				<article className={styles.empty}>
					<p>{isAdmin ? adminEmpty : viewerEmpty}</p>
					{createButton}
				</article>
			) : (
				<div className={styles.workspace}>
					<nav className={styles.funnelRail} aria-label="Saved funnels">
						{isAdmin && (
							<button type="button" className={styles.newFunnel} onClick={() => setEditor({ funnel: null })}>
								New funnel
							</button>
						)}
						<ul className={styles.funnelList}>
							{funnels.map((funnel) => (
								<li key={funnel.id}>
									<button
										type="button"
										className={styles.funnelButton}
										aria-pressed={funnel.id === selected?.id}
										onClick={() => setSelectedId(funnel.id)}
									>
										{funnel.displayName}
									</button>
								</li>
							))}
						</ul>
					</nav>
					<div className={styles.reportPane}>
						<div className={styles.picker}>
							<label className={styles.pickerLabel} htmlFor="funnel-select">
								Funnel
							</label>
							<div className={styles.pickerRow}>
								<select
									id="funnel-select"
									value={selected?.id ?? ""}
									onChange={(event) => setSelectedId(event.target.value)}
								>
									{funnels.map((funnel) => (
										<option key={funnel.id} value={funnel.id}>
											{funnel.displayName}
										</option>
									))}
								</select>
								{isAdmin && (
									<button type="button" onClick={() => setEditor({ funnel: null })}>
										New funnel
									</button>
								)}
							</div>
						</div>
						<SelectFilters value={filters} onChange={setFilters} dimensions={filterDimensions} />
						{selected && (
							<article className={styles.card}>
								<header className={styles.reportHeader}>
									<div className={styles.reportTitle}>
										<h2>{selected.displayName}</h2>
										{!countsHidden && first > 0 && (
											<p className={styles.completion}>
												<strong>{formatPercent(completion)}</strong> of visitor groups completed all{" "}
												{selected.steps.length} steps · {formatMetricVal(last, "views")} groups
											</p>
										)}
										{report.isLoading && report.report && (
											<span className={styles.updating} role="status" aria-label="Updating report">
												<LoadingSpinner immediate />
											</span>
										)}
									</div>
									{isAdmin && (
										<div className={styles.reportActions}>
											<button type="button" className="button-ghost" onClick={() => setEditor({ funnel: selected })}>
												Edit
											</button>
											<button type="button" className="button-ghost" onClick={() => setDeleting(selected)}>
												Delete
											</button>
										</div>
									)}
								</header>
								{report.hiddenEvents ? (
									<p className={styles.message}>This funnel uses custom events, which are hidden for this project.</p>
								) : report.error ? (
									<p className={styles.message}>Failed to load the funnel report.</p>
								) : report.isLoading && !report.report ? (
									<div className={styles.loading} role="status" aria-label="Loading funnel report">
										<LoadingSpinner />
									</div>
								) : first === 0 ? (
									<p className={styles.message}>No visitor groups matched the first step in this range.</p>
								) : (
									<div className={styles.tableScroll}>
										<table className={styles.table}>
											<caption className={styles.caption}>
												Visitor groups that reached each step of {selected.displayName}
											</caption>
											<thead>
												<tr>
													<th scope="col">Step</th>
													<th scope="col" className={styles.number}>
														Visitor groups
													</th>
													<th scope="col" className={styles.number}>
														From step 1
													</th>
													<th scope="col" className={styles.number}>
														Drop-off
													</th>
												</tr>
											</thead>
											<tbody>
												{selected.steps.map((step, index) => {
													const count = counts[index] ?? 0;
													const previous = counts[index - 1] ?? 0;
													const conversion = first > 0 ? (count / first) * 100 : undefined;
													const dropoff =
														index === 0 || previous === 0 ? undefined : ((previous - count) / previous) * 100;
													return (
														<tr key={index}>
															<th scope="row" className={styles.stepCell}>
																<div className={styles.stepHeading}>
																	<span className={styles.stepNumber}>{index + 1}</span>
																	<StepIcon step={step} />
																	<span className={styles.stepName} title={stepName(step)}>
																		{stepName(step)}
																	</span>
																	<span className={styles.stepKind}>
																		{step.type === "page" ? "Page" : "Custom event"}
																	</span>
																	{step.type === "page" && (
																		<span className={styles.match}>
																			{matchLabels[(step.matchType ?? "equal") as keyof typeof matchLabels]}
																		</span>
																	)}
																</div>
																<div className={styles.barTrack} aria-hidden="true">
																	<div className={styles.bar} style={{ width: `${(count / first) * 100}%` }} />
																</div>
															</th>
															<td className={styles.number}>{formatMetricVal(count, "views")}</td>
															<td className={styles.number}>
																{conversion === undefined ? "—" : formatPercent(conversion)}
															</td>
															<td className={styles.number}>{dropoff === undefined ? "—" : formatPercent(dropoff)}</td>
														</tr>
													);
												})}
											</tbody>
										</table>
									</div>
								)}
								<p className={styles.legend}>
									Visitor groups, not people. Groups reset once a day, so steps must happen before that reset to count.
								</p>
							</article>
						)}
					</div>
				</div>
			)}
			{editor && (
				<FunnelDialog
					key={editor.funnel?.id ?? "new"}
					projectId={project.id}
					project={project}
					range={range}
					funnel={editor.funnel}
					customEventsHidden={customEventsHidden}
					open
					onOpenChange={(open) => !open && setEditor(null)}
					onSaved={(id) => {
						if (id) setSelectedId(id);
					}}
				/>
			)}
			{deleting && (
				<DeleteFunnelDialog
					projectId={project.id}
					funnel={deleting}
					open
					onOpenChange={(open) => !open && setDeleting(null)}
					onDeleted={() => setSelectedId(null)}
				/>
			)}
		</div>
	);
};
