import { DataQuery, DataSourceJsonData } from '@grafana/data';

export interface ConsoleQuery extends DataQuery {
  stream?: string;
}

export const defaultQuery: Partial<ConsoleQuery> = {
  stream: 'tasks',
};

/**
 * These are options configured for each DataSource instance.
 */
export interface DataSourceOptions extends DataSourceJsonData {
  url?: string;
}
