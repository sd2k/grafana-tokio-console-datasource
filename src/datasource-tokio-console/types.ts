import { DataQuery, DataSourceJsonData } from '@grafana/data';

export interface ConsoleQuery extends DataQuery {
  streamPath?: string;
}

export const defaultQuery: Partial<ConsoleQuery> = {
  streamPath: 'tasks',
};

/**
 * These are options configured for each DataSource instance.
 */
export interface DataSourceOptions extends DataSourceJsonData {
  url?: string;
}
