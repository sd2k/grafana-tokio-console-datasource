import { DataQuery, DataSourceJsonData } from '@grafana/data';

export enum ConsolePathName {
  Tasks = 'tasks',
  TaskDetails = 'task',
  Resources = 'resources',
};

interface PartialConsoleQuery extends DataQuery {
  path?: ConsolePathName;
}

export interface TasksConsolePath extends PartialConsoleQuery {
  path?: ConsolePathName.Tasks;
};

export interface ResourcesConsolePath extends PartialConsoleQuery {
  path?: ConsolePathName.Resources;
}

export interface TaskDetailsConsolePath extends PartialConsoleQuery {
  path?: ConsolePathName.TaskDetails;
  taskId?: number;
}

export type ConsoleQuery = TasksConsolePath | ResourcesConsolePath | TaskDetailsConsolePath;

export const defaultQuery: Partial<ConsoleQuery> = {
  path: ConsolePathName.Tasks,
};

/**
 * These are options configured for each DataSource instance.
 */
export interface DataSourceOptions extends DataSourceJsonData {
  url?: string;
}
