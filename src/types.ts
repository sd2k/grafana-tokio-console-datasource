import { DataQuery, DataSourceJsonData } from '@grafana/data';

// Regular datasource queries.

export enum ConsolePathName {
  Tasks = 'tasks',
  TaskDetails = 'task',
  TaskHistogram = 'taskHistogram',
  Resources = 'resources',
}

interface PartialConsoleQuery extends DataQuery {
  path?: ConsolePathName;
}

export interface TasksConsolePath extends PartialConsoleQuery {
  path?: ConsolePathName.Tasks;
}

export interface ResourcesConsolePath extends PartialConsoleQuery {
  path?: ConsolePathName.Resources;
}

export interface TaskDetailsConsolePath extends PartialConsoleQuery {
  path?: ConsolePathName.TaskDetails;
  taskId?: number;
  rawTaskId?: string;
}

export interface TaskHistogramConsolePath extends PartialConsoleQuery {
  path?: ConsolePathName.TaskHistogram;
  taskId?: number;
  rawTaskId?: string;
}

export type ConsoleQuery = TasksConsolePath | ResourcesConsolePath | TaskDetailsConsolePath | TaskHistogramConsolePath;

export const defaultQuery: Partial<ConsoleQuery> = {
  path: ConsolePathName.Tasks,
};

// Variable queries.

export enum VariableQueryPathName {
  Tasks = 'tasks',
}

export interface VariableQuery {
  path?: VariableQueryPathName;
}

/**
 * These are options configured for each DataSource instance.
 */
export interface DataSourceOptions extends DataSourceJsonData {
  url?: string;
}
