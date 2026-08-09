import { Injectable } from '@nestjs/common';
import { NotificationRepository } from './notification.repository';

@Injectable()
export class NotificationService {
  constructor(private readonly repo: NotificationRepository) {}

  async findById(id: number) {
    return this.repo.findById(id);
  }

  async create(data: any) {
    return this.repo.insert(data);
  }

  async delete(id: number) {
    return this.repo.remove(id);
  }
}
